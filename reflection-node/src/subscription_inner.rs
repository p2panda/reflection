use std::mem::take;
use std::ops::{Deref, DerefMut, Drop};
use std::sync::Arc;

use chrono::Utc;
use p2panda_core::{
    Body, Header,
    cbor::{decode_cbor, encode_cbor},
};
use p2panda_net::{FromNetwork, Network, ToNetwork};
use p2panda_stream::IngestExt;
use tokio::{
    sync::{RwLock, mpsc},
    task::{AbortHandle, spawn},
};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tracing::{error, info, warn};

use crate::author_tracker::{AuthorMessage, AuthorTracker};
use crate::document::{DocumentError, DocumentId, SubscribableDocument};
use crate::ephemerial_operation::EphemerialOperation;
use crate::node_inner::MessageType;
use crate::node_inner::NodeInner;
use crate::operation::{LogType, ReflectionExtensions};
use crate::persistent_operation::PersistentOperation;

pub struct SubscriptionInner<T> {
    tx: RwLock<Option<mpsc::Sender<ToNetwork>>>,
    pub(crate) node: Arc<NodeInner>,
    pub(crate) id: DocumentId,
    pub(crate) document: Arc<T>,
    abort_handles: RwLock<Vec<AbortHandle>>,
}

impl<T> Drop for SubscriptionInner<T> {
    fn drop(&mut self) {
        for handle in self.abort_handles.get_mut() {
            handle.abort();
        }
    }
}

impl<T: SubscribableDocument + 'static> SubscriptionInner<T> {
    pub fn new(node: Arc<NodeInner>, id: DocumentId, document: Arc<T>) -> Arc<Self> {
        Arc::new(SubscriptionInner {
            tx: RwLock::new(None),
            node,
            id,
            abort_handles: RwLock::new(Vec::new()),
            document,
        })
    }

    pub async fn spawn_network_monitor(&self) {
        // We need to hold a read lock to the network, so that the network won't be dropped
        // or shutdown.
        let mut notify = Some(self.node.network_notifier.notified());
        let mut network_guard = Some(self.node.network.read().await);

        let (tx, abort_handles) = if let Some(network) = network_guard.as_ref().unwrap().deref() {
            setup_network(
                &self.node,
                network,
                self.id,
                &self.document,
            )
            .await
        } else {
            (None, Vec::new())
        };

        *self.tx.write().await = tx;
        *self.abort_handles.write().await = abort_handles;

        loop {
            if let Some(notify) = notify {
                notify.await;
            }

            let mut abort_handles_guard = self.abort_handles.write().await;
            let mut tx_guard = self.tx.write().await;

            let old_tx = take(tx_guard.deref_mut());
            let old_abort_handles = take(abort_handles_guard.deref_mut());

            teardown_network(&self.node, &self.id, old_tx, old_abort_handles).await;
            // Release network lock and get a new one, so that the network can be change between them
            network_guard.take();
            notify = Some(self.node.network_notifier.notified());
            network_guard = Some(self.node.network.read().await);

            let (tx, abort_handles) = if let Some(network) = network_guard.as_ref().unwrap().deref()
            {
                setup_network(
                    &self.node,
                    network,
                    self.id,
                    &self.document,
                )
                .await
            } else {
                (None, Vec::new())
            };

            *tx_guard = tx;
            *abort_handles_guard = abort_handles;
        }
    }

    pub async fn unsubscribe(&self) -> Result<(), DocumentError> {
        let mut tx_guard = self.tx.write().await;
        let mut abort_handles_guard = self.abort_handles.write().await;

        let tx = take(tx_guard.deref_mut());
        let abort_handles = take(abort_handles_guard.deref_mut());

        self.node
            .document_store
            .set_last_accessed_for_document(&self.id, Some(Utc::now()))
            .await?;

        teardown_network(&self.node, &self.id, tx, abort_handles).await;

        Ok(())
    }

    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let operation =
                // Append one operation to our "ephemeral" delta log.
                self.node.operation_store
                    .create_operation(
                        &self.node.private_key,
                        LogType::Delta,
                        Some(self.id),
                        Some(&data),
                        false,
                    )
                    .await?;

        info!("Delta operation sent for document with id {}", self.id);

        if let Some(tx) = self.tx.read().await.as_ref() {
            let bytes = encode_cbor(&MessageType::Persistent(PersistentOperation::new(
                operation,
            )))?;

            // Broadcast operation on gossip overlay.
            tx.send(ToNetwork::Message { bytes }).await?;
        }

        Ok(())
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        // Append an operation to our "snapshot" log and set the prune flag to
        // true. This will remove previous snapshots.
        //
        // Snapshots are not broadcasted on the gossip overlay as they would be
        // too large. Peers will sync them up when they join the document.
        self.node
            .operation_store
            .create_operation(
                &self.node.private_key,
                LogType::Snapshot,
                Some(self.id),
                Some(&data),
                true,
            )
            .await?;

        // Append an operation to our "ephemeral" delta log and set the prune
        // flag to true.
        //
        // This signals removing all previous "delta" operations now. This is
        // some sort of garbage collection whenever we snapshot. Snapshots
        // already contain all history, there is no need to keep duplicate
        // "delta" data around.
        let operation = self
            .node
            .operation_store
            .create_operation(
                &self.node.private_key,
                LogType::Delta,
                Some(self.id),
                None,
                true,
            )
            .await?;

        info!("Snapshot saved for document with id {}", self.id);

        if let Some(tx) = self.tx.read().await.as_ref() {
            let bytes = encode_cbor(&MessageType::Persistent(PersistentOperation::new(
                operation,
            )))?;

            // Broadcast operation on gossip overlay.
            tx.send(ToNetwork::Message { bytes }).await?;
        }

        Ok(())
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        if let Some(tx) = self.tx.read().await.as_ref() {
            let operation = EphemerialOperation::new(data, &self.node.private_key);
            let bytes = encode_cbor(&MessageType::Ephemeral(operation))?;
            tx.send(ToNetwork::Message { bytes }).await?;
        }

        Ok(())
    }

    /// Set the name for a given document
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), DocumentError> {
        self.node
            .document_store
            .set_name_for_document(&self.id, name)
            .await?;

        Ok(())
    }
}

async fn setup_network(
    node: &Arc<NodeInner>,
    network: &Network<DocumentId>,
    document_id: DocumentId,
    document: &Arc<impl SubscribableDocument + 'static>,
) -> (Option<mpsc::Sender<ToNetwork>>, Vec<AbortHandle>) {
    let mut abort_handles = Vec::with_capacity(3);

    let (document_tx, mut document_rx, gossip_ready) = match network.subscribe(document_id).await {
        Ok(result) => result,
        Err(error) => {
            warn!(
                "Failed to setup network for subscription to document {}: {error}",
                document_id
            );
            return (None, abort_handles);
        }
    };

    let (persistent_tx, persistent_rx) =
        mpsc::channel::<(Header<ReflectionExtensions>, Option<Body>, Vec<u8>)>(128);

    let author_tracker = AuthorTracker::new(node.clone(), document.clone(), document_tx.clone());

    let author_tracker_clone = author_tracker.clone();
    let document_clone = document.clone();
    let abort_handle = spawn(async move {
        while let Some(event) = document_rx.recv().await {
            match event {
                FromNetwork::GossipMessage { bytes, .. } => match decode_cbor(&bytes[..]) {
                    Ok(MessageType::Ephemeral(operation)) => {
                        if let Some((author, body)) = operation.validate_and_unpack() {
                            document_clone.ephemeral_bytes_received(author, body);
                        } else {
                            warn!("Got ephemeral operation with a bad signature");
                        }
                    }
                    Ok(MessageType::AuthorEphemeral(operation)) => {
                        if let Some((author, body)) = operation.validate_and_unpack() {
                            match AuthorMessage::try_from(&body[..]) {
                                Ok(message) => {
                                    author_tracker_clone.received(message, author).await;
                                }
                                Err(error) => {
                                    warn!("Failed to deserialize AuthorMessage: {error}");
                                }
                            }
                        } else {
                            warn!("Got internal ephemeral operation with a bad signature");
                        }
                    }
                    Ok(MessageType::Persistent(operation)) => {
                        match operation.validate_and_unpack(document_id) {
                            Ok(data) => {
                                persistent_tx.send(data).await.unwrap();
                            }
                            Err(err) => {
                                error!("Failed to unpack operation: {err}");
                            }
                        }
                    }
                    Err(err) => {
                        error!("Failed to decode gossip message: {err}");
                    }
                },
                FromNetwork::SyncMessage {
                    header, payload, ..
                } => match PersistentOperation::from_serialized(header, payload)
                    .validate_and_unpack(document_id)
                {
                    Ok(data) => persistent_tx.send(data).await.unwrap(),
                    Err(err) => {
                        error!("Failed to unpack operation: {err}");
                    }
                },
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    let stream = ReceiverStream::new(persistent_rx);

    // Ingest does multiple things for us:
    //
    // - Validate operation- and log integrity and authenticity
    // - De-duplicate already known operations
    // - Out-of-order buffering
    // - Pruning when flag is set
    // - Persist operation in store
    let mut stream = stream
        // NOTE(adz): The persisting part should happen later, we want to check the payload on
        // application layer first. In general "ingest" does too much at once and is
        // inflexible. Related issue: https://github.com/p2panda/p2panda/issues/696
        .ingest(node.operation_store.clone_inner(), 128)
        .filter_map(|result| match result {
            Ok(operation) => Some(operation),
            Err(err) => {
                error!("ingesting operation failed: {err}");
                None
            }
        });

    let node = node.clone();
    let document_clone = document.clone();
    // Send checked and ingested operations for this document to application layer.
    let abort_handle = spawn(async move {
        while let Some(operation) = stream.next().await {
            // When we discover a new author we need to add them to our document store.
            if let Err(error) = node
                .document_store
                .add_author(&document_id, &operation.header.public_key)
                .await
            {
                error!("Can't store author to database: {error}");
            }

            // Forward the payload up to the app.
            if let Some(body) = operation.body {
                document_clone.bytes_received(operation.header.public_key, body.to_bytes());
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    let abort_handle = spawn(async move {
        // Only start track authors once we have joined the gossip overlay
        if let Err(error) = gossip_ready.await {
            error!("Failed to join the gossip overlay: {error}");
        }

        author_tracker.spawn().await;
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    info!("Network subscription set up for document {}", document_id);

    (Some(document_tx), abort_handles)
}

async fn teardown_network(
    node: &Arc<NodeInner>,
    document_id: &DocumentId,
    tx: Option<mpsc::Sender<ToNetwork>>,
    abort_handles: Vec<AbortHandle>,
) {
    for handle in abort_handles {
        handle.abort();
    }

    // Send good bye message to the network
    if let Some(tx) = tx {
        if let Err(error) = AuthorMessage::Bye.send(&tx, &node.private_key).await {
            error!("Failed to sent bye message to the network: {error}");
        }

        info!(
            "Network subscription torn down for document {}",
            document_id
        );
    }
}
