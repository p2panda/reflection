use std::collections::HashMap;
use std::hash::Hash as StdHash;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::{Extension, Hash, PrivateKey, PruneFlag, PublicKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration, ToNetwork, TopicId};
use p2panda_store::MemoryStore;
use p2panda_stream::{DecodeExt, IngestExt};
use p2panda_sync::TopicQuery;
use p2panda_sync::log_sync::{LogSyncProtocol, TopicLogMap};
use serde::{Deserialize, Serialize};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::OnceCell;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

use crate::operation::{
    AardvarkExtensions, create_operation, decode_gossip_message, encode_gossip_operation,
};

#[derive(Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct TextDocument([u8; 32]);

impl TopicQuery for TextDocument {}

impl TopicId for TextDocument {
    fn id(&self) -> [u8; 32] {
        self.0
    }
}

type LogId = TextDocument;

#[derive(Clone, Debug)]
struct TextDocumentStore {
    inner: Arc<RwLock<TextDocumentStoreInner>>,
}

impl TextDocumentStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TextDocumentStoreInner {
                authors: HashMap::new(),
            })),
        }
    }

    pub fn write(&self) -> RwLockWriteGuard<TextDocumentStoreInner> {
        self.inner.write().expect("acquire write lock")
    }
}

#[derive(Clone, Debug)]
struct TextDocumentStoreInner {
    authors: HashMap<PublicKey, Vec<TextDocument>>,
}

#[async_trait]
impl TopicLogMap<TextDocument, LogId> for TextDocumentStore {
    async fn get(&self, topic: &TextDocument) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let authors = &self.inner.read().unwrap().authors;
        let mut result = HashMap::<PublicKey, Vec<LogId>>::new();

        for (public_key, text_documents) in authors {
            if text_documents.contains(topic) {
                result
                    .entry(*public_key)
                    .and_modify(|logs| logs.push(topic.clone()))
                    .or_insert(vec![topic.clone()]);
            }
        }

        Some(result)
    }
}

pub struct Network {
    inner: Arc<NetworkInner>,
}

struct NetworkInner {
    runtime: Runtime,
    #[allow(dead_code)]
    shutdown_tx: OnceCell<oneshot::Sender<()>>,
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

        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("backend runtime ready to spawn tasks");

        Network {
            inner: Arc::new(NetworkInner {
                operations_store,
                documents_store,
                network: OnceCell::new(),
                private_key: OnceCell::new(),
                shutdown_tx: OnceCell::new(),
                runtime,
            }),
        }
    }

    pub fn run(&self, private_key: PrivateKey, network_id: Hash) {
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let sync = LogSyncProtocol::new(
            self.inner.documents_store.clone(),
            self.inner.operations_store.clone(),
        );
        let sync_config = SyncConfiguration::<TextDocument>::new(sync);

        self.inner
            .private_key
            .set(private_key.clone())
            .expect("network can be run only once");

        self.inner
            .shutdown_tx
            .set(shutdown_tx)
            .expect("network can be run only once");

        let network_inner_clone = self.inner.clone();
        std::thread::spawn(move || {
            let network_inner_clone2 = network_inner_clone.clone();
            network_inner_clone.runtime.block_on(async move {
                network_inner_clone2
                    .network
                    .get_or_init(|| async {
                        NetworkBuilder::new(network_id.into())
                            .private_key(private_key)
                            .discovery(LocalDiscovery::new())
                            .gossip(GossipConfig {
                                // @TODO(adz): This is a temporary workaround to account for Automerge giving
                                // us surprisingly fairly large payloads which break the default gossip message
                                // size limit given by iroh-gossip (4092 bytes).
                                //
                                // This especially happens if another peer edits a document for the first time
                                // which already contains some text, even if it's just adding one single
                                // character. It's also surprising that the 4kb limit is reached even if the
                                // text itself is less than ca. 100 characters long.
                                //
                                // I believe we can fix this by understanding better how Automerge's "diffs"
                                // are made and possibily using more low-level methods of their library to
                                // really only send the actual changed text.
                                //
                                // Related issue: https://github.com/p2panda/aardvark/issues/11
                                max_message_size: 512_000,
                                ..Default::default()
                            })
                            .sync(sync_config)
                            .build()
                            .await
                            .expect("network spawning")
                    })
                    .await;

                shutdown_rx.await.unwrap();
            });
        });
    }

    pub fn get_or_create_document(
        &self,
        document_id: Hash,
    ) -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let document_id = TextDocument(document_id.into());

        let (to_network, mut from_app) = mpsc::channel::<Vec<u8>>(512);
        let (to_app, from_network) = mpsc::channel(512);

        let network_inner = self.inner.clone();
        self.inner.runtime.spawn(async move {
            // Wait for the network to be started
            let network = network_inner
                .network
                .get_or_init(|| async { unreachable!("network not running") })
                .await;
            let (topic_tx, topic_rx, ready) = network
                .subscribe(document_id.clone())
                .await
                .expect("subscribe to topic");

            tokio::task::spawn(async move {
                let _ = ready.await;
                println!("network joined!");
            });

            let document_id_clone = document_id.clone();
            let stream = ReceiverStream::new(topic_rx);
            let stream = stream.filter_map(|event| match event {
                FromNetwork::GossipMessage { bytes, .. } => match decode_gossip_message(&bytes) {
                    Ok(result) => Some(result),
                    Err(err) => {
                        eprintln!("could not decode gossip message: {err}");
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
                        eprintln!("decode operation error: {err}");
                        None
                    }
                })
                .ingest(network_inner.operations_store.clone(), 128)
                .filter_map(|result| match result {
                    Ok(operation) => Some(operation),
                    Err(err) => {
                        eprintln!("ingest operation error: {err}");
                        None
                    }
                });

            let documents_store = network_inner.documents_store.clone();
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                // Process the operations and forward application messages to app layer.
                while let Some(operation) = stream.next().await {
                    let prune_flag: PruneFlag = operation.header.extract().unwrap_or_default();
                    println!(
                        "received operation from {}, seq_num={}, prune_flag={}",
                        operation.header.public_key,
                        operation.header.seq_num,
                        prune_flag.is_set(),
                    );

                    // When we discover a new author we need to add them to our "document store".
                    {
                        let mut write_lock = documents_store.write();
                        write_lock
                            .authors
                            .entry(operation.header.public_key)
                            .and_modify(|documents| {
                                if !documents.contains(&document_id_clone) {
                                    documents.push(document_id_clone.clone());
                                }
                            })
                            .or_insert(vec![document_id_clone.clone()]);
                    };

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
                .expect("no private key set")
                .clone();
            // Task for handling events coming from the application layer.
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                while let Some(bytes) = from_app.recv().await {
                    // @TODO: set prune flag from the frontend.
                    let prune_flag = false;

                    // Create the p2panda operations with application message as payload.
                    let (header, body) = create_operation(
                        &mut operations_store,
                        &private_key,
                        document_id.clone(),
                        Some(&bytes),
                        prune_flag,
                    )
                    .await?;

                    println!(
                        "created operation seq_num={}, prune_flag={}, payload_size={}",
                        header.seq_num,
                        prune_flag,
                        bytes.len(),
                    );

                    let encoded_gossip_operation = encode_gossip_operation(header, body)?;

                    // Broadcast operation on gossip overlay.
                    topic_tx
                        .send(ToNetwork::Message {
                            bytes: encoded_gossip_operation,
                        })
                        .await?;
                }

                Ok(())
            });
        });

        (to_network, from_network)
    }
}
