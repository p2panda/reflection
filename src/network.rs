use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::{Extension, Hash, PrivateKey, PruneFlag, PublicKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration};
use p2panda_net::{ToNetwork, TopicId};
use p2panda_store::MemoryStore;
use p2panda_sync::log_sync::LogSyncProtocol;
use p2panda_sync::{TopicMap, TopicQuery};
use serde::{Deserialize, Serialize};
use tokio::runtime::Builder;

#[derive(Clone, Default, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
struct TextDocument([u8; 32]);

impl TopicQuery for TextDocument {}

impl TopicId for TextDocument {
    fn id(&self) -> [u8; 32] {
        self.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AarvdarkExtensions {
    prune_flag: PruneFlag,
}

impl Extension<PruneFlag> for AarvdarkExtensions {
    fn extract(&self) -> Option<PruneFlag> {
        Some(self.prune_flag.clone())
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

pub fn run() -> Result<(Sender<Vec<u8>>, Receiver<Vec<u8>>)> {
    let (to_network, from_app) = std::sync::mpsc::channel::<Vec<u8>>();
    let (to_app, from_network) = std::sync::mpsc::channel();

    let rt_handle: JoinHandle<Result<()>> = std::thread::spawn(|| {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("backend runtime ready to spawn tasks");

        runtime.block_on(async {
            let network_id = Hash::new(b"aardvark <3");
            let private_key = PrivateKey::new();

            let operations_store = MemoryStore::<LogId, AarvdarkExtensions>::new();
            let documents_store = TextDocumentStore::new();

            let sync = LogSyncProtocol::new(documents_store, operations_store);
            let sync_config = SyncConfiguration::<TextDocument>::new(sync);

            let mut network = NetworkBuilder::new(*network_id.as_bytes())
                .sync(sync_config)
                .private_key(private_key)
                .discovery(LocalDiscovery::new()?)
                .build()
                .await?;

            let test_document = TextDocument(Hash::new(b"my first doc <3").into());
            let (topic_tx, mut topic_rx, ready) = network.subscribe(test_document).await?;

            tokio::task::spawn(async move {
                while let Some(message) = topic_rx.recv().await {
                    println!("New message from network");

                    let bytes = match message {
                        FromNetwork::GossipMessage {
                            bytes,
                            delivered_from,
                        } => bytes,
                        FromNetwork::SyncMessage {
                            header,
                            payload,
                            delivered_from,
                        } => payload.expect("all messages have a payload"),
                    };

                    // 1) decode operation
                    // 2) check who the author is and add them to our TextDocumentStore
                    // 3) persist the operation
                    // 4) forward the payload onto the app

                    to_app.send(bytes).expect("can send on channel");
                }
            });

            tokio::task::spawn(async move {
                while let Ok(bytes) = from_app.recv() {
                    println!("New message from app");

                    // 1) encode operation
                    // 2) persist operation
                    // 3) forward operation to the network

                    topic_tx.send(ToNetwork::Message { bytes }).await;
                }
            });

            Ok(())
        })
    });

    Ok((to_network, from_network))
}
