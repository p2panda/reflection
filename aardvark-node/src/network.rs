use std::sync::Arc;

use anyhow::Result;
use p2panda_core::{Hash, PrivateKey, PruneFlag};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration, ToNetwork};
use p2panda_store::MemoryStore;
use p2panda_stream::{DecodeExt, IngestExt};
use p2panda_sync::log_sync::LogSyncProtocol;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tracing::{debug, error};

use crate::operation::{
    create_operation, decode_gossip_message, encode_gossip_operation, AardvarkExtensions,
};
use crate::store::{LogId, TextDocumentStore};
use crate::topic::TextDocument;

pub struct Network {
    inner: Arc<NetworkInner>,
}

struct NetworkInner {
    runtime: Runtime,
    operations_store: MemoryStore<LogId, AardvarkExtensions>,
    documents_store: TextDocumentStore,
    network: OnceCell<p2panda_net::Network<TextDocument>>,
    private_key: OnceCell<PrivateKey>,
}

impl Default for Network {
    fn default() -> Self {
        Network::new()
    }
}

impl Network {
    pub fn new() -> Self {
        let operations_store = MemoryStore::<LogId, AardvarkExtensions>::new();
        let documents_store = TextDocumentStore::new();

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("single-threaded tokio runtime");

        Network {
            inner: Arc::new(NetworkInner {
                operations_store,
                documents_store,
                network: OnceCell::new(),
                private_key: OnceCell::new(),
                runtime,
            }),
        }
    }

    pub fn run(&self, private_key: PrivateKey, network_id: Hash) {
        let sync = LogSyncProtocol::new(
            self.inner.documents_store.clone(),
            self.inner.operations_store.clone(),
        );
        let sync_config = SyncConfiguration::<TextDocument>::new(sync);

        self.inner
            .private_key
            .set(private_key.clone())
            .expect("network can be run only once");

        let network_inner_clone = self.inner.clone();
        self.inner.runtime.spawn(async move {
            network_inner_clone
                .network
                .get_or_init(|| async {
                    NetworkBuilder::new(network_id.into())
                        .private_key(private_key)
                        .discovery(LocalDiscovery::new())
                        .gossip(GossipConfig {
                            // FIXME: This is a temporary workaround to account for larger delta
                            // patches (for example when the user Copy & Pastes a big chunk of
                            // text).
                            //
                            // Related issue: https://github.com/p2panda/aardvark/issues/24
                            max_message_size: 512_000,
                            ..Default::default()
                        })
                        .sync(sync_config)
                        .build()
                        .await
                        .expect("network spawning")
                })
                .await;
        });
    }

    pub fn shutdown(&self) {
        let network = self.inner.network.get().expect("network running").clone();
        self.inner.runtime.block_on(async move {
            network.shutdown().await.expect("network to shutdown");
        });
    }

    pub fn create_document(
        &self,
    ) -> Result<(Hash, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)> {
        let mut operations_store = self.inner.operations_store.clone();
        let private_key = self.inner.private_key.get().expect("private key to be set");

        let (header, _body) = self.inner.runtime.block_on(async {
            create_operation(&mut operations_store, private_key, None, None, false).await
        })?;

        let document: TextDocument = header.extension().expect("document id from our own logs");
        let document_id = (&document).into();

        let (tx, rx) = self.subscribe(document)?;

        Ok((document_id, tx, rx))
    }

    pub fn join_document(
        &self,
        document_id: Hash,
    ) -> Result<(mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)> {
        let document = document_id.into();
        let (tx, rx) = self.subscribe(document)?;
        Ok((tx, rx))
    }

    fn subscribe(
        &self,
        document: TextDocument,
    ) -> Result<(mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)> {
        let (to_network, mut from_app) = mpsc::channel::<Vec<u8>>(512);
        let (to_app, from_network) = mpsc::channel(512);

        let network_inner = self.inner.clone();
        self.inner.runtime.spawn(async move {
            // Wait for the network to be started
            let network = network_inner
                .network
                .get_or_init(|| async { unreachable!("network not running") })
                .await;

            let (document_tx, document_rx, _gossip_ready) = network
                .subscribe(document.clone())
                .await
                .expect("subscribe to topic");

            let stream = ReceiverStream::new(document_rx);
            let stream = stream.filter_map(|event| match event {
                FromNetwork::GossipMessage { bytes, .. } => match decode_gossip_message(&bytes) {
                    Ok(result) => Some(result),
                    Err(err) => {
                        error!("could not decode gossip message: {err}");
                        None
                    }
                },
                FromNetwork::SyncMessage {
                    header, payload, ..
                } => Some((header, payload)),
            });

            // Decode and ingest the p2panda operations.
            let mut stream = stream
                .decode()
                .filter_map(|result| match result {
                    Ok(operation) => Some(operation),
                    Err(err) => {
                        error!("decode operation error: {err}");
                        None
                    }
                })
                .ingest(network_inner.operations_store.clone(), 128)
                .filter_map(|result| match result {
                    Ok(operation) => Some(operation),
                    Err(err) => {
                        error!("ingest operation error: {err}");
                        None
                    }
                });

            let documents_store = network_inner.documents_store.clone();
            let document_clone = document.clone();
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                // Process the operations and forward application messages to app layer.
                while let Some(operation) = stream.next().await {
                    let prune_flag: PruneFlag = operation.header.extension().unwrap_or_default();
                    debug!(
                        "received operation from {}, seq_num={}, prune_flag={}",
                        operation.header.public_key,
                        operation.header.seq_num,
                        prune_flag.is_set(),
                    );

                    // When we discover a new author we need to add them to our "document store".
                    documents_store
                        .add_author(document_clone.clone(), operation.header.public_key)
                        .await?;

                    // Forward the payload up to the app.
                    to_app
                        .send(
                            operation
                                .body
                                .expect("all operations have a body")
                                .to_bytes(),
                        )
                        .await?;
                }

                Ok(())
            });

            let mut operations_store = network_inner.operations_store.clone();
            let private_key = network_inner
                .private_key
                .get()
                .expect("private key to be set")
                .clone();
            // Task for handling events coming from the application layer.
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                while let Some(bytes) = from_app.recv().await {
                    // TODO: set prune flag from the frontend.
                    let prune_flag = false;

                    // Create the p2panda operations with application message as payload.
                    let (header, body) = create_operation(
                        &mut operations_store,
                        &private_key,
                        Some(document.clone()),
                        Some(&bytes),
                        prune_flag,
                    )
                    .await?;

                    debug!(
                        "created operation seq_num={}, prune_flag={}, payload_size={}",
                        header.seq_num,
                        prune_flag,
                        bytes.len(),
                    );

                    let encoded_gossip_operation = encode_gossip_operation(header, body)?;

                    // Broadcast operation on gossip overlay.
                    document_tx
                        .send(ToNetwork::Message {
                            bytes: encoded_gossip_operation,
                        })
                        .await?;
                }

                Ok(())
            });
        });

        Ok((to_network, from_network))
    }
}
