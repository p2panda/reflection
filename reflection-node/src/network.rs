use crate::document::DocumentId;
use crate::operation::{ReflectionExtensions, decode_gossip_message, encode_gossip_operation};
use crate::store::OperationStore;
use anyhow::Result;
use p2panda_core::{Hash, Operation, PrivateKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration, SystemEvent, ToNetwork};
use p2panda_stream::{DecodeExt, IngestExt};
use std::collections::HashMap;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;

#[derive(Debug)]
pub struct Network {
    operation_store: OperationStore,
    network: p2panda_net::Network<DocumentId>,
    document_tx: RwLock<HashMap<DocumentId, mpsc::Sender<ToNetwork>>>,
}

const RELAY_URL: &str = "https://wiese.liebechaos.org/";
const BOOTSTRAP_NODE_ID: &str = "466872e38dcff721077830eb0feb7bb333072ab335a7793f6f60586733ccfe27";

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
            .relay(RELAY_URL.parse().expect("valid relay URL"), false, 0)
            .direct_address(
                BOOTSTRAP_NODE_ID.parse().expect("valid node ID"),
                vec![],
                None,
            )
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

    pub async fn subscribe<Fut>(
        &self,
        document: DocumentId,
        f: impl Fn(Operation<ReflectionExtensions>) -> Fut + Send + 'static,
    ) -> Result<()>
    where
        Fut: Future<Output = ()> + Send,
    {
        // Join a gossip overlay with peers who are interested in the same document and start sync
        // with them.
        let (document_tx, document_rx, _gossip_ready) = self.network.subscribe(document).await?;

        {
            let mut store = self.document_tx.write().await;
            store.insert(document.clone(), document_tx);
        }

        let stream = ReceiverStream::new(document_rx);

        // Incoming gossip payloads have a slightly different shape than sync. We convert them
        // here to follow the p2panda operation tuple of a "header" and separate "body".
        let stream = stream.filter_map(|event| match event {
            FromNetwork::GossipMessage { bytes, .. } => match decode_gossip_message(&bytes) {
                Ok(result) => Some(result),
                Err(err) => {
                    error!("decoding gossip message failed: {err}");
                    None
                }
            },
            FromNetwork::SyncMessage {
                header, payload, ..
            } => Some((header, payload)),
        });

        // Decode p2panda operations (they are encoded in CBOR).
        let stream = stream.decode().filter_map(|result| match result {
            Ok(operation) => Some(operation),
            Err(err) => {
                error!("decoding operation failed: {err}");
                None
            }
        });

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
        let document_tx = {
            self.document_tx
                .read()
                .await
                .get(document)
                .cloned()
                .expect("Not subscribed to document with id {document_id}")
        };

        let encoded_gossip_operation = encode_gossip_operation(operation.header, operation.body)?;
        document_tx
            .send(ToNetwork::Message {
                bytes: encoded_gossip_operation,
            })
            .await?;

        Ok(())
    }
}
