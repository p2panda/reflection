use std::sync::Arc;

use crate::document::{DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::ephemerial_operation::EphemerialOperation;
use crate::operation::{ReflectionExtensions, validate_operation};
use crate::operation_store::OperationStore;
use crate::persistent_operation::PersistentOperation;
use crate::store::DocumentStore;
use anyhow::Result;
use p2panda_core::{
    Body, Hash, Header, Operation, PrivateKey,
    cbor::{decode_cbor, encode_cbor},
};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration, ToNetwork};
use p2panda_stream::IngestExt;
use std::collections::HashMap;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum MessageType {
    Persistent(PersistentOperation),
    Ephemeral(EphemerialOperation),
}

#[derive(Debug)]
pub struct NodeInner {
    pub(crate) runtime: Runtime,
    pub(crate) operation_store: OperationStore,
    pub(crate) document_store: DocumentStore,
    pub(crate) private_key: PrivateKey,
    pub(crate) network: p2panda_net::Network<DocumentId>,
    pub(crate) document_tx: RwLock<HashMap<DocumentId, mpsc::Sender<ToNetwork>>>,
}

//const RELAY_URL: &str = "https://staging-euw1-1.relay.iroh.network/";
//const BOOTSTRAP_NODE_ID: &str = "d825a2f929f935efcd6889bed5c3f5510b40f014969a729033d3fb7e33b97dbe";

impl NodeInner {
    pub async fn new(
        runtime: Runtime,
        network_id: Hash,
        private_key: PrivateKey,
        sync_config: SyncConfiguration<DocumentId>,
        operation_store: OperationStore,
        document_store: DocumentStore,
    ) -> Result<Self> {
        let network = NetworkBuilder::new(network_id.into())
            .private_key(private_key.clone())
            .discovery(LocalDiscovery::new())
            // NOTE(glyph): Internet networking is disabled until we can fix the
            // more-than-two-peers gossip issue.
            //
            //.relay(RELAY_URL.parse().expect("valid relay URL"), false, 0)
            //.direct_address(
            //    BOOTSTRAP_NODE_ID.parse().expect("valid node ID"),
            //    vec![],
            //    None,
            //)
            .gossip(GossipConfig {
                // FIXME: This is a temporary workaround to account for larger delta patches (for
                // example when the user Copy & Pastes a big chunk of text).
                //
                // Related issue: https://github.com/p2panda/reflection/issues/24
                max_message_size: 512_000,
            })
            .sync(sync_config)
            .build()
            .await?;

        Ok(Self {
            runtime,
            operation_store,
            document_store,
            private_key,
            network,
            document_tx: RwLock::new(HashMap::new()),
        })
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.network.clone().shutdown().await?;
        Ok(())
    }

    pub async fn subscribe(
        self: Arc<Self>,
        document_id: DocumentId,
        document: Arc<impl SubscribableDocument + 'static>,
    ) -> Result<Subscription, DocumentError> {
        self.document_store.add_document(&document_id).await?;
        // Add ourselves as an author to the document store.
        self.document_store
            .add_author(&document_id, &self.private_key.public_key())
            .await?;
        let stored_operations = self
            .document_store
            .operations_for_document(&self.operation_store, &document_id)
            .await?;

        for operation in stored_operations {
            // Send all stored operation bytes to the app,
            // it doesn't matter if the app already knows some or all of them
            if let Some(body) = operation.body {
                document.bytes_received(operation.header.public_key, body.to_bytes());
            }
        }

        // Join a gossip overlay with peers who are interested in the same document and start sync
        // with them.
        let (document_tx, mut document_rx, _gossip_ready) =
            self.network.subscribe(document_id).await?;

        {
            let mut store = self.document_tx.write().await;
            store.insert(document_id.clone(), document_tx.clone());
        }

        let (persistent_tx, persistent_rx) =
            mpsc::channel::<(Header<ReflectionExtensions>, Option<Body>, Vec<u8>)>(128);

        let document_clone = document.clone();
        let subscription_abort_handle = self
            .runtime
            .spawn(async move {
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
                            Ok(MessageType::Persistent(operation)) => match operation.unpack() {
                                Ok(data) => {
                                    persistent_tx.send(data).await.unwrap();
                                }
                                Err(err) => {
                                    error!("Failed to unpack operation: {err}");
                                }
                            },
                            Err(err) => {
                                error!("Failed to decode gossip message: {err}");
                            }
                        },
                        FromNetwork::SyncMessage {
                            header, payload, ..
                        } => match PersistentOperation::from_serialized(header, payload).unpack() {
                            Ok(data) => persistent_tx.send(data).await.unwrap(),
                            Err(err) => {
                                error!("Failed to unpack operation: {err}");
                            }
                        },
                    }
                }
            })
            .abort_handle();

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
            .ingest(self.operation_store.clone_inner(), 128)
            .filter_map(|result| match result {
                Ok(operation) => Some(operation),
                Err(err) => {
                    error!("ingesting operation failed: {err}");
                    None
                }
            });

        let inner_clone = self.clone();
        let document_clone = document.clone();
        // Send checked and ingested operations for this document to application layer.
        let subscription2_abort_handle = self
            .runtime
            .spawn(async move {
                while let Some(operation) = stream.next().await {
                    // Process the operations and forward application messages to app layer. This is where
                    // we "materialize" our application state from incoming "application events".
                    // Validation for our custom "document" extension.
                    if let Err(err) = validate_operation(&operation, &document_id) {
                        warn!(
                            public_key = %operation.header.public_key,
                            seq_num = %operation.header.seq_num,
                            "{err}"
                        );
                        return;
                    }

                    // When we discover a new author we need to add them to our document store.
                    if let Err(error) = inner_clone
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

        info!("Subscribed to document {document_id}");

        Ok(Subscription {
            tx: document_tx,
            id: document_id,
            node: self,
            abort_handles: vec![subscription_abort_handle, subscription2_abort_handle],
        })
    }

    pub async fn unsubscribe(&self, document_id: &DocumentId) -> Result<()> {
        self.document_tx.write().await.remove(document_id);

        Ok(())
    }

    /// Send operations to the gossip overlay for `document`.
    ///
    /// This will panic if the `document` wasn't subscribed to.
    pub async fn send_operation(
        &self,
        document: &DocumentId,
        operation: Operation<ReflectionExtensions>,
    ) -> Result<()> {
        let document_tx = {
            self.document_tx
                .read()
                .await
                .get(document)
                .cloned()
                .expect("Not subscribed to document with id {document_id}")
        };

        let bytes = encode_cbor(&MessageType::Persistent(PersistentOperation::new(
            operation,
        )))?;
        document_tx.send(ToNetwork::Message { bytes }).await?;

        Ok(())
    }

    /// Send ephemeral data to the gossip overlay for `document`.
    ///
    /// This will panic if the `document` wasn't subscribed to.
    pub async fn send_ephemeral(
        &self,
        document: &DocumentId,
        operation: EphemerialOperation,
    ) -> Result<()> {
        let document_tx = {
            self.document_tx
                .read()
                .await
                .get(document)
                .cloned()
                .expect("Not subscribed to document with id {document_id}")
        };

        let bytes = encode_cbor(&MessageType::Ephemeral(operation))?;
        document_tx.send(ToNetwork::Message { bytes }).await?;

        Ok(())
    }
}
