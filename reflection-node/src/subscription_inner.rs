use std::mem::take;
use std::ops::{Deref, DerefMut, Drop};
use std::sync::Arc;

use chrono::Utc;
use p2panda_core::Operation;
use p2panda_core::{
    Body, Header,
    cbor::{decode_cbor, encode_cbor},
};
use p2panda_net::{TopicId, gossip::GossipHandle};
use p2panda_stream::IngestExt;
use p2panda_sync::protocols::TopicLogSyncEvent as Event;
use tokio::{
    sync::{RwLock, mpsc},
    task::{AbortHandle, spawn},
};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tracing::{error, info, warn};

use crate::author_tracker::{AuthorMessage, AuthorTracker};
use crate::ephemerial_operation::EphemerialOperation;
use crate::network::Network;
use crate::node_inner::MessageType;
use crate::node_inner::NodeInner;
use crate::operation::{LogType, ReflectionExtensions};
use crate::topic::{SubscribableTopic, SubscriptionError, TopicError};

pub type SyncHandle =
    p2panda_net::sync::SyncHandle<Operation<ReflectionExtensions>, Event<ReflectionExtensions>>;

pub struct SubscriptionInner<T> {
    ephemeral_tx: RwLock<Option<GossipHandle>>,
    tx: RwLock<Option<SyncHandle>>,
    pub(crate) node: Arc<NodeInner>,
    pub(crate) id: TopicId,
    pub(crate) subscribable_topic: Arc<T>,
    author_tracker: Arc<AuthorTracker<T>>,
    abort_handles: RwLock<Vec<AbortHandle>>,
}

impl<T> Drop for SubscriptionInner<T> {
    fn drop(&mut self) {
        for handle in self.abort_handles.get_mut() {
            handle.abort();
        }
    }
}

impl<T: SubscribableTopic + 'static> SubscriptionInner<T> {
    pub fn new(node: Arc<NodeInner>, id: TopicId, subscribable_topic: Arc<T>) -> Self {
        let author_tracker = AuthorTracker::new(node.clone(), subscribable_topic.clone());
        SubscriptionInner {
            tx: RwLock::new(None),
            ephemeral_tx: RwLock::new(None),
            node,
            id,
            abort_handles: RwLock::new(Vec::new()),
            subscribable_topic,
            author_tracker,
        }
    }

    pub async fn spawn_network_monitor(&self) {
        // We need to hold a read lock to the network, so that the network won't be dropped
        // or shutdown.
        let mut notify = Some(self.node.network_notifier.notified());
        let mut network_guard = Some(self.node.network.read().await);

        let (tx, ephemeral_tx, abort_handles) =
            if let Some(network) = network_guard.as_ref().unwrap().deref() {
                match setup_network(
                    &self.node,
                    network,
                    self.id,
                    &self.subscribable_topic,
                    &self.author_tracker,
                )
                .await
                {
                    Ok((sync_handle, gossip_handle, abort_handles)) => {
                        (Some(sync_handle), Some(gossip_handle), abort_handles)
                    }
                    Err(error) => {
                        self.subscribable_topic.error(error);
                        (None, None, Vec::new())
                    }
                }
            } else {
                (None, None, Vec::new())
            };

        *self.tx.write().await = tx;
        *self.ephemeral_tx.write().await = ephemeral_tx;
        *self.abort_handles.write().await = abort_handles;

        loop {
            if let Some(notify) = notify {
                notify.await;
            }

            let mut abort_handles_guard = self.abort_handles.write().await;
            let mut tx_guard = self.tx.write().await;
            let mut ephemeral_tx_guard = self.ephemeral_tx.write().await;

            let old_tx = take(tx_guard.deref_mut());
            let old_ephemeral_tx = take(ephemeral_tx_guard.deref_mut());
            let old_abort_handles = take(abort_handles_guard.deref_mut());

            teardown_network(
                &self.id,
                &self.author_tracker,
                old_tx,
                old_ephemeral_tx,
                old_abort_handles,
            )
            .await;
            // Release network lock and get a new one, so that the network can be change between them
            network_guard.take();
            notify = Some(self.node.network_notifier.notified());
            network_guard = Some(self.node.network.read().await);

            let (tx, ephemeral_tx, abort_handles) =
                if let Some(network) = network_guard.as_ref().unwrap().deref() {
                    match setup_network(
                        &self.node,
                        network,
                        self.id,
                        &self.subscribable_topic,
                        &self.author_tracker,
                    )
                    .await
                    {
                        Ok((sync_handle, gossip_handle, abort_handles)) => {
                            (Some(sync_handle), Some(gossip_handle), abort_handles)
                        }
                        Err(error) => {
                            self.subscribable_topic.error(error);
                            (None, None, Vec::new())
                        }
                    }
                } else {
                    (None, None, Vec::new())
                };

            *tx_guard = tx;
            *ephemeral_tx_guard = ephemeral_tx;
            *abort_handles_guard = abort_handles;
        }
    }

    pub async fn unsubscribe(&self) -> Result<(), TopicError> {
        let mut tx_guard = self.tx.write().await;
        let mut ephemeral_tx_guard = self.ephemeral_tx.write().await;
        let mut abort_handles_guard = self.abort_handles.write().await;

        let tx = take(tx_guard.deref_mut());
        let ephemeral_tx = take(ephemeral_tx_guard.deref_mut());
        let abort_handles = take(abort_handles_guard.deref_mut());

        self.node
            .topic_store
            .set_last_accessed_for_topic(&self.id, Some(Utc::now()))
            .await?;

        teardown_network(
            &self.id,
            &self.author_tracker,
            tx,
            ephemeral_tx,
            abort_handles,
        )
        .await;

        Ok(())
    }

    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), TopicError> {
        let operation =
                // Append one operation to our "ephemeral" delta log.
                self.node.operation_store
                    .create_operation(
                        &self.node.private_key,
                        LogType::Delta,
                        self.id,
                        Some(&data),
                        false,
                    )
                    .await?;

        info!(
            "Delta operation sent for topic with id {}",
            hex::encode(self.id)
        );

        if let Some(tx) = self.tx.read().await.as_ref() {
            tx.publish(operation).await?;
        }

        Ok(())
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), TopicError> {
        // Append an operation to our "snapshot" log and set the prune flag to
        // true. This will remove previous snapshots.
        //
        // Snapshots are not broadcasted on the gossip overlay as they would be
        // too large. Peers will sync them up when they join the topic.
        self.node
            .operation_store
            .create_operation(
                &self.node.private_key,
                LogType::Snapshot,
                self.id,
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
            .create_operation(&self.node.private_key, LogType::Delta, self.id, None, true)
            .await?;

        info!("Snapshot saved for topic with id {}", hex::encode(self.id));

        if let Some(tx) = self.tx.read().await.as_ref() {
            tx.publish(operation).await?;
        }

        Ok(())
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), TopicError> {
        if let Some(ephemeral_tx) = self.ephemeral_tx.read().await.as_ref() {
            let operation = EphemerialOperation::new(data, &self.node.private_key);
            let bytes = encode_cbor(&MessageType::Ephemeral(operation))?;
            ephemeral_tx.publish(bytes).await?;
        }

        Ok(())
    }

    /// Set the name for a given topic
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), TopicError> {
        self.node
            .topic_store
            .set_name_for_topic(&self.id, name)
            .await?;

        Ok(())
    }
}

async fn setup_network<T: SubscribableTopic + 'static>(
    node: &Arc<NodeInner>,
    network: &Network,
    id: TopicId,
    subscribable_topic: &Arc<T>,
    author_tracker: &Arc<AuthorTracker<T>>,
) -> Result<(SyncHandle, GossipHandle, Vec<AbortHandle>), SubscriptionError> {
    let mut abort_handles = Vec::with_capacity(3);

    let stream = network.log_sync.stream(id, true).await?;
    let mut topic_rx = stream.subscribe().await?;
    let topic_tx = stream;

    let (persistent_tx, persistent_rx) =
        mpsc::channel::<(Header<ReflectionExtensions>, Option<Body>, Vec<u8>)>(128);

    let abort_handle = spawn(async move {
        while let Some(event) = topic_rx.next().await {
            let event = match event {
                Ok(event) => event,
                Err(error) => {
                    error!("Error while receiving sync message: {error}");
                    continue;
                }
            };
            match event.event() {
                Event::Operation(operation) => {
                    match validate_and_unpack(operation.as_ref().to_owned(), id) {
                        Ok(data) => {
                            persistent_tx.send(data).await.unwrap();
                        }
                        Err(err) => {
                            error!("Failed to unpack operation: {err}");
                        }
                    }
                }
                _ => {
                    // TODO: Handle sync events
                }
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    let ephemeral_stream = network.gossip.stream(id).await?;
    let mut ephemeral_rx = ephemeral_stream.subscribe();
    let ephemeral_tx = ephemeral_stream;

    author_tracker.set_topic_tx(Some(ephemeral_tx)).await;

    let author_tracker_clone = author_tracker.clone();
    let subscribable_topic_clone = subscribable_topic.clone();
    let abort_handle = spawn(async move {
        while let Some(bytes) = ephemeral_rx.next().await {
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(error) => {
                    error!("Error while receiving ephemeral message: {error}");
                    continue;
                }
            };
            match decode_cbor(&bytes[..]) {
                Ok(MessageType::Ephemeral(operation)) => {
                    if let Some((author, body)) = operation.validate_and_unpack() {
                        subscribable_topic_clone.ephemeral_bytes_received(author, body);
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
                Err(err) => {
                    error!("Failed to decode gossip message: {err}");
                }
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
    let subscribable_topic_clone = subscribable_topic.clone();
    // Send checked and ingested operations for this topic to application layer.
    let abort_handle = spawn(async move {
        while let Some(operation) = stream.next().await {
            // When we discover a new author we need to add them to our topic store.
            if let Err(error) = node
                .topic_store
                .add_author(&id, &operation.header.public_key)
                .await
            {
                error!("Can't store author to database: {error}");
            }

            // Forward the payload up to the app.
            if let Some(body) = operation.body {
                subscribable_topic_clone
                    .bytes_received(operation.header.public_key, body.to_bytes());
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);
    let author_tracker_clone = author_tracker.clone();
    let abort_handle = spawn(async move {
        author_tracker_clone.spawn().await;
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    info!("Network subscription set up for topic {}", hex::encode(id));

    let ephemeral_tx = network.gossip.stream(id).await?;

    Ok((topic_tx, ephemeral_tx, abort_handles))
}

async fn teardown_network<T: SubscribableTopic + 'static>(
    id: &TopicId,
    author_tracker: &Arc<AuthorTracker<T>>,
    tx: Option<SyncHandle>,
    ephemeral_tx: Option<GossipHandle>,
    abort_handles: Vec<AbortHandle>,
) {
    for handle in abort_handles {
        handle.abort();
    }

    author_tracker.set_topic_tx(None).await;

    if tx.is_some() {
        info!(
            "Network subscription torn down for topic {}",
            hex::encode(id)
        );
    }
    drop(tx);
    drop(ephemeral_tx);
}

type OperationWithRawHeader = (Header<ReflectionExtensions>, Option<Body>, Vec<u8>);

#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    #[error(transparent)]
    Cbor(#[from] p2panda_core::cbor::DecodeError),
    #[error("Operation with invalid topic id")]
    InvalidTopicId,
}

fn validate_and_unpack(
    operation: p2panda_core::Operation<ReflectionExtensions>,
    id: TopicId,
) -> Result<OperationWithRawHeader, UnpackError> {
    let p2panda_core::Operation::<ReflectionExtensions> { header, body, .. } = operation;

    let Some(operation_id): Option<TopicId> = header.extension() else {
        return Err(UnpackError::InvalidTopicId);
    };

    if operation_id != id {
        return Err(UnpackError::InvalidTopicId);
    }

    Ok((header.clone(), body, header.to_bytes()))
}
