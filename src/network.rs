use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::{Extension, Hash, PrivateKey, PruneFlag, PublicKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration};
use p2panda_net::{ToNetwork, TopicId};
use p2panda_store::MemoryStore;
use p2panda_stream::{DecodeExt, IngestExt};
use p2panda_sync::log_sync::LogSyncProtocol;
use p2panda_sync::{TopicMap, TopicQuery};
use serde::{Deserialize, Serialize};
use tokio::runtime::Builder;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::operation::{
    create_operation, decode_gossip_message, encode_gossip_operation, AardvarkExtensions,
};

#[derive(Clone, Default, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
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
impl TopicMap<TextDocument, HashMap<PublicKey, Vec<LogId>>> for TextDocumentStore {
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

pub fn run() -> Result<(
    oneshot::Sender<()>,
    mpsc::Sender<Vec<u8>>,
    mpsc::Receiver<Vec<u8>>,
)> {
    let (to_network, mut from_app) = mpsc::channel::<Vec<u8>>(512);
    let (to_app, from_network) = mpsc::channel(512);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    std::thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("backend runtime ready to spawn tasks");

        runtime.block_on(async move {
            let network_id = Hash::new(b"aardvark <3");
            let document_id = TextDocument(Hash::new(b"my first doc <3").into());

            let private_key = PrivateKey::new();

            let mut operations_store = MemoryStore::<LogId, AardvarkExtensions>::new();
            let documents_store = TextDocumentStore::new();
            documents_store
                .write()
                .authors
                .insert(private_key.public_key(), vec![document_id.clone()]);

            let sync = LogSyncProtocol::new(documents_store.clone(), operations_store.clone());
            let sync_config = SyncConfiguration::<TextDocument>::new(sync);

            let network = NetworkBuilder::new(network_id.into())
                .sync(sync_config)
                .private_key(private_key.clone())
                .discovery(LocalDiscovery::new().expect("local discovery service"))
                .build()
                .await
                .expect("network spawning");

            let (topic_tx, topic_rx, ready) = network
                .subscribe(document_id.clone())
                .await
                .expect("subscribe to topic");

            tokio::task::spawn(async move {
                let _ = ready.await;
                println!("network joined!");
            });

            // Task for handling operations arriving from the network.
            let operations_store_clone = operations_store.clone();
            let document_id_clone = document_id.clone();
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                let stream = ReceiverStream::new(topic_rx);

                let stream = stream.filter_map(|event| match event {
                    FromNetwork::GossipMessage { bytes, .. } => match decode_gossip_message(&bytes)
                    {
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
                    .ingest(operations_store_clone, 128);

                // Process the operations and forward application messages to app layer.
                while let Some(message) = stream.next().await {
                    match message {
                        Ok(operation) => {
                            let prune_flag: PruneFlag =
                                operation.header.extract().unwrap_or_default();
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
                        Err(err) => {
                            eprintln!("could not ingest message: {err}");
                        }
                    }
                }

                Ok(())
            });

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
                        "created operation seq_num={}, prune_flag={}",
                        header.seq_num, prune_flag
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

            shutdown_rx.await.unwrap();
        });
    });

    Ok((shutdown_tx, to_network, from_network))
}
