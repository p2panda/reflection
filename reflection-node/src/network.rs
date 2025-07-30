use crate::document::DocumentId;
use crate::ephemerial_operation::EphemerialOperation;
use crate::operation::ReflectionExtensions;
use crate::persistent_operation::PersistentOperation;
use crate::store::OperationStore;
use anyhow::Result;
use p2panda_core::{
    Body, Hash, Header, Operation, PrivateKey,
    cbor::{decode_cbor, encode_cbor},
};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration, SystemEvent, ToNetwork};
use p2panda_stream::IngestExt;
use std::collections::HashMap;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum MessageType {
    Persistent(PersistentOperation),
    Ephemeral(EphemerialOperation),
}

#[derive(Debug)]
pub struct Network {
    operation_store: OperationStore,
    network: p2panda_net::Network<DocumentId>,
    document_tx: RwLock<HashMap<DocumentId, mpsc::Sender<ToNetwork>>>,
}

//const RELAY_URL: &str = "https://staging-euw1-1.relay.iroh.network/";
//const BOOTSTRAP_NODE_ID: &str = "d825a2f929f935efcd6889bed5c3f5510b40f014969a729033d3fb7e33b97dbe";

impl Network {
    pub async fn spawn(
        network_id: Hash,
        private_key: PrivateKey,
        sync_config: SyncConfiguration<DocumentId>,
        operation_store: OperationStore,
    ) -> Result<Self> {
        let network = NetworkBuilder::new(network_id.into())
            .private_key(private_key)
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
            operation_store,
            network,
            document_tx: RwLock::new(HashMap::new()),
        })
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.network.clone().shutdown().await?;
        Ok(())
    }

    pub async fn subscribe<Fut, Fut2>(
        &self,
        document: DocumentId,
        f: impl Fn(Operation<ReflectionExtensions>) -> Fut + Send + 'static,
        f_ephemeral: impl Fn(EphemerialOperation) -> Fut2 + Send + 'static,
    ) -> Result<()>
    where
        Fut: Future<Output = ()> + Send,
        Fut2: Future<Output = ()> + Send,
    {
        // Join a gossip overlay with peers who are interested in the same document and start sync
        // with them.
        let (document_tx, mut document_rx, _gossip_ready) =
            self.network.subscribe(document).await?;

        {
            let mut store = self.document_tx.write().await;
            store.insert(document.clone(), document_tx);
        }

        let (persistent_tx, persistent_rx) =
            mpsc::channel::<(Header<ReflectionExtensions>, Option<Body>, Vec<u8>)>(128);

        tokio::task::spawn(async move {
            while let Some(event) = document_rx.recv().await {
                match event {
                    FromNetwork::GossipMessage { bytes, .. } => match decode_cbor(&bytes[..]) {
                        Ok(MessageType::Ephemeral(operation)) => {
                            f_ephemeral(operation).await;
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
        });

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
            .ingest(self.operation_store.clone(), 128)
            .filter_map(|result| match result {
                Ok(operation) => Some(operation),
                Err(err) => {
                    error!("ingesting operation failed: {err}");
                    None
                }
            });

        // Send checked and ingested operations for this document to application layer.
        tokio::task::spawn(async move {
            while let Some(operation) = stream.next().await {
                f(operation).await;
            }
        });

        Ok(())
    }

    pub async fn unsubscribe(&self, document_id: &DocumentId) -> Result<()> {
        self.document_tx.write().await.remove(document_id);

        Ok(())
    }

    pub async fn subscribe_events<Fut>(
        &self,
        f: impl Fn(SystemEvent<DocumentId>) -> Fut + Send + 'static,
    ) -> Result<()>
    where
        Fut: Future<Output = ()> + Send,
    {
        let mut events = self.network.events().await?;

        tokio::task::spawn(async move {
            while let Ok(event) = events.recv().await {
                f(event).await;
            }
        });

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
        info!(id = %operation.header.hash(), seq_num = %operation.header.seq_num, "send operation");

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
