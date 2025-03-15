use anyhow::Result;
use p2panda_core::{Hash, Operation, PrivateKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration, ToNetwork};
use p2panda_stream::{DecodeExt, IngestExt};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing::error;

use crate::document::Document;
use crate::operation::{AardvarkExtensions, decode_gossip_message, encode_gossip_operation};
use crate::store::OperationStore;

const RELAY_URL: &str = "https://staging-euw1-1.relay.iroh.network/";

const BOOTSTRAP_NODE_ID: &str = "265c1d43f994777af5333b6a82918bc612dff19a42c1325509d65277bc66ff01";

#[derive(Clone, Debug)]
pub struct Network {
    operation_store: OperationStore,
    network: p2panda_net::Network<Document>,
}

impl Network {
    pub async fn spawn(
        network_id: Hash,
        private_key: PrivateKey,
        sync_config: SyncConfiguration<Document>,
        operation_store: OperationStore,
    ) -> Result<Self> {
        let network = NetworkBuilder::new(network_id.into())
            .private_key(private_key)
            .discovery(LocalDiscovery::new())
            .relay(RELAY_URL.parse().expect("relay url is fine"), false, 0)
            .direct_address(
                BOOTSTRAP_NODE_ID.parse().expect("public key is fine"),
                vec![],
                None,
            )
            .gossip(GossipConfig {
                // FIXME: This is a temporary workaround to account for larger delta patches (for
                // example when the user Copy & Pastes a big chunk of text).
                //
                // Related issue: https://github.com/p2panda/aardvark/issues/24
                max_message_size: 512_000,
            })
            .sync(sync_config)
            .build()
            .await?;

        Ok(Self {
            operation_store,
            network,
        })
    }

    pub async fn shutdown(self) -> Result<()> {
        self.network.shutdown().await?;
        Ok(())
    }

    pub async fn subscribe(
        &self,
        document: Document,
    ) -> Result<(
        mpsc::Sender<Operation<AardvarkExtensions>>,
        mpsc::Receiver<Operation<AardvarkExtensions>>,
    )> {
        let (to_network, mut from_app) = mpsc::channel::<Operation<AardvarkExtensions>>(128);
        let (to_app, from_network) = mpsc::channel(128);

        // Join a gossip overlay with peers who are interested in the same document and start sync
        // with them.
        let (document_tx, document_rx, _gossip_ready) = self.network.subscribe(document).await?;

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
        let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
            while let Some(operation) = stream.next().await {
                to_app.send(operation).await?;
            }
            Ok(())
        });

        // Receive operations from application layer and forward them into gossip overlay for this
        // document.
        let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
            while let Some(operation) = from_app.recv().await {
                let encoded_gossip_operation =
                    encode_gossip_operation(operation.header, operation.body)?;
                document_tx
                    .send(ToNetwork::Message {
                        bytes: encoded_gossip_operation,
                    })
                    .await?;
            }
            Ok(())
        });

        Ok((to_network, from_network))
    }
}
