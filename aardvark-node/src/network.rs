use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use futures_util::FutureExt;
use iroh_gossip::proto::Config as GossipConfig;
use p2panda_core::{cbor, Extension, Hash, PrivateKey, PruneFlag};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration};
use p2panda_net::{Network, ToNetwork};
use p2panda_store::MemoryStore;
use p2panda_stream::{DecodeExt, IngestExt};
use p2panda_sync::log_sync::LogSyncProtocol;
use tokio::runtime::Builder;
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::document::{ShortCode, TextDocumentStore};
use crate::operation::{
    create_operation, decode_gossip_message, encode_gossip_operation, init_document,
    AardvarkExtensions, LogId,
};
use crate::topics::{AardvarkTopics, DiscoveryCode, TextDocument};

pub enum FromApp {
    CreateDocument,
    SubscribeToDocument(ShortCode),
    HandleMessage(Vec<u8>),
}

pub enum ToApp {
    SubscriptionSuccess(TextDocument),
    MessageReceived(Vec<u8>),
}

pub fn run() -> Result<(
    oneshot::Sender<()>,
    mpsc::Sender<FromApp>,
    mpsc::Receiver<ToApp>,
)> {
    let (from_app_tx, from_app_rx) = mpsc::channel::<FromApp>(512);
    let (to_app_tx, to_app_rx) = mpsc::channel(512);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    std::thread::spawn(move || {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("backend runtime ready to spawn tasks");

        runtime.block_on(async move {
            let network_id = Hash::new(b"aardvark <3");
            let private_key = PrivateKey::new();
            println!("my public key: {}", private_key.public_key());

            let operations_store = MemoryStore::<LogId, AardvarkExtensions>::new();
            let documents_store = TextDocumentStore::default();
            let sync = LogSyncProtocol::new(documents_store.clone(), operations_store.clone());
            let sync_config = SyncConfiguration::<AardvarkTopics>::new(sync);

            let network = NetworkBuilder::new(network_id.into())
                .private_key(private_key.clone())
                .discovery(LocalDiscovery::new().expect("local discovery service"))
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
                .expect("network spawning");

            let node = Node::new(
                private_key,
                network,
                operations_store,
                documents_store,
                to_app_tx.clone(),
            );

            let _join_handle: JoinHandle<Result<()>> = node.run(from_app_rx).await;

            shutdown_rx.await.unwrap();
        });
    });

    Ok((shutdown_tx, from_app_tx, to_app_rx))
}

/// Struct encapsulating core application logic such as; discovering and announcing documents,
/// subscribing and unsubscribing to documents, ingesting application message bytes.
pub struct Node {
    private_key: PrivateKey,
    network: Network<AardvarkTopics>,
    operations_store: MemoryStore<LogId, AardvarkExtensions>,
    documents_store: TextDocumentStore,
    discovered_documents: HashMap<ShortCode, TextDocument>,
    to_app_tx: mpsc::Sender<ToApp>,
    state: TopicState,
}

#[derive(Default)]
pub struct TopicState {
    topic_tx: Option<mpsc::Sender<ToNetwork>>,
    unsubscribe_tx: Option<oneshot::Sender<()>>,
    join_handle: Option<JoinHandle<Result<()>>>,
    document: Option<TextDocument>,
}

impl Node {
    pub fn new(
        private_key: PrivateKey,
        network: Network<AardvarkTopics>,
        operations_store: MemoryStore<LogId, AardvarkExtensions>,
        documents_store: TextDocumentStore,
        to_app_tx: mpsc::Sender<ToApp>,
    ) -> Self {
        Node {
            private_key,
            network,
            operations_store,
            documents_store,
            discovered_documents: Default::default(),
            to_app_tx,
            state: Default::default(),
        }
    }

    pub async fn run(mut self, mut from_app: mpsc::Receiver<FromApp>) -> JoinHandle<Result<()>> {
        tokio::task::spawn(async move {
            while let Some(message) = from_app.recv().await {
                match message {
                    FromApp::CreateDocument => self.create().await?,
                    FromApp::SubscribeToDocument(short_code) => {
                        let document = match self.discovered_documents.get(&short_code) {
                            Some(document) => document.clone(),
                            None => {
                                let document = self.discover(short_code).await?;
                                self.announce(&document).await?;
                                document
                            }
                        };
                        self.subscribe(&document).await?;
                        self.to_app_tx
                            .send(ToApp::SubscriptionSuccess(document.clone()))
                            .await?;
                    }
                    FromApp::HandleMessage(bytes) => self.handle_application_bytes(bytes).await?,
                }
            }

            Ok(())
        })
    }

    /// Handle application message bytes for a document.
    async fn handle_application_bytes(&mut self, bytes: Vec<u8>) -> Result<()> {
        let Some(document) = &self.state.document else {
            return Err(anyhow::anyhow!("no document subscribed to"));
        };

        // @TODO: set prune flag from the frontend.
        let prune_flag = false;

        // Create the p2panda operations with application message as payload.
        let (header, body) = create_operation(
            &mut self.operations_store,
            &self.private_key,
            document,
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
        let Some(ref mut topic_tx) = self.state.topic_tx else {
            return Err(anyhow::anyhow!(
                "no document subscribed to: topic_tx is None"
            ));
        };

        topic_tx
            .send(ToNetwork::Message {
                bytes: encoded_gossip_operation,
            })
            .await?;

        Ok(())
    }

    async fn create(&mut self) -> Result<()> {
        let document = init_document(&mut self.operations_store, &self.private_key).await?;

        self.discovered_documents
            .insert(document.short_code(), document.clone());

        self.subscribe(&document).await?;
        self.announce(&document).await?;

        self.to_app_tx
            .send(ToApp::SubscriptionSuccess(document.clone()))
            .await?;

        Ok(())
    }

    /// Discover a document based on its `ShortCode`.
    ///
    /// Note: this method returns a future which may never resolve if no peers in our network are
    /// announcing his document.
    async fn discover(&mut self, short_code: ShortCode) -> Result<TextDocument> {
        let (_, mut topic_rx, _) = self
            .network
            .subscribe(AardvarkTopics::DiscoveryCode(DiscoveryCode(short_code)))
            .await?;

        if let FromNetwork::GossipMessage { bytes, .. } =
            topic_rx.recv().await.expect("channel to be open")
        {
            let document: TextDocument = cbor::decode_cbor(&bytes[..])?;
            self.discovered_documents
                .insert(document.short_code(), document.clone());
            Ok(document)
        } else {
            Err(anyhow::format_err!(
                "Unexpected message received via sync on discovery channel"
            ))
        }
    }

    /// Spawn a task which announces documents on a discovery gossip overlay.
    async fn announce(&self, document: &TextDocument) -> Result<()> {
        let document = document.to_owned();

        let (discovery_topic_tx, _, _) = self
            .network
            .subscribe(AardvarkTopics::DiscoveryCode(DiscoveryCode(
                document.short_code(),
            )))
            .await?;

        let mut announce_interval = tokio::time::interval(Duration::from_secs(5));

        // Task for announcing documents.
        tokio::task::spawn(async move {
            // @TODO: would be nice to have a "cancel announce" oneshot channel here so we can
            // stop announcing topics.
            loop {
                announce_interval.tick().await;
                discovery_topic_tx
                    .send(ToNetwork::Message {
                        bytes: cbor::encode_cbor(&document).expect("can encode text document"),
                    })
                    .await
                    .expect("topic channel to be able to receive messages");
            }
        });

        Ok(())
    }

    /// Subscribe to a document and spawn a task to handle messages arriving from the network.
    async fn subscribe(&mut self, document: &TextDocument) -> Result<()> {
        if let Some(tx) = self.state.unsubscribe_tx.take() {
            tx.send(()).unwrap(); //@TODO: error handling
            let task_result = self.state.join_handle.take().unwrap().await?;
            task_result?;
        }

        // Insert any document we subscribe to into the document store so that sync sessions can
        // correctly occur.
        self.documents_store
            .write()
            .authors
            .insert(self.private_key.public_key(), vec![document.clone()]);

        let (topic_tx, topic_rx, ready) = self
            .network
            .subscribe(AardvarkTopics::TextDocument(document.clone()))
            .await?;

        let (unsubscribe_tx, unsubscribe_rx) = oneshot::channel();
        let unsubscribe_rx = unsubscribe_rx.shared();

        let document_clone = document.clone();
        tokio::task::spawn(async move {
            let _ = ready.await;
            println!("network joined for document: {document_clone:?}");
        });

        let document_clone = document.clone();
        let documents_store = self.documents_store.clone();
        let operations_store = self.operations_store.clone();
        let to_app_tx = self.to_app_tx.clone();

        // Task for handling operations arriving from the network.
        let join_handle: JoinHandle<Result<()>> = tokio::task::spawn(async move {
            let stream = ReceiverStream::new(topic_rx);

            let stream = stream.filter_map(|event| {
                println!("{event:?}");
                match event {
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
                }
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
                .ingest(operations_store, 128);

            // Process the operations and forward application messages to app layer.
            loop {
                let unsubscribe_rx_clone = unsubscribe_rx.clone();
                select! {
                    _ = unsubscribe_rx_clone => {
                        return Ok(())
                    }
                    Some(message) = stream.next() => {
                        println!("{message:?}");
                        match message {
                            Ok(operation) => {
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
                                            if !documents.contains(&document_clone) {
                                                documents.push(document_clone.clone());
                                            }
                                        })
                                        .or_insert(vec![document_clone.clone()]);
                                };

                                // Forward the payload up to the app.
                                if let Some(body) = operation.body {
                                    to_app_tx
                                    .send(ToApp::MessageReceived(body.to_bytes()))
                                    .await?;
                                }
                            }
                            Err(err) => {
                                eprintln!("could not ingest message: {err}");
                            }
                        }
                    }
                }
            }
        });

        self.state = TopicState {
            topic_tx: Some(topic_tx),
            unsubscribe_tx: Some(unsubscribe_tx),
            document: Some(document.clone()),
            join_handle: Some(join_handle),
        };

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use p2panda_core::{Hash, PrivateKey};
    use p2panda_net::{NetworkBuilder, NetworkId, SyncConfiguration};
    use p2panda_store::MemoryStore;
    use p2panda_sync::log_sync::LogSyncProtocol;
    use tokio::sync::mpsc;

    use crate::document::TextDocumentStore;
    use crate::network::{FromApp, LogId, Node};
    use crate::operation::{init_document, AardvarkExtensions};
    use crate::topics::AardvarkTopics;

    use super::ToApp;

    async fn test_node(network_id: NetworkId, to_app_tx: mpsc::Sender<ToApp>) -> Node {
        let private_key = PrivateKey::new();

        let operations_store = MemoryStore::<LogId, AardvarkExtensions>::new();

        let documents_store = TextDocumentStore::default();
        let sync = LogSyncProtocol::new(documents_store.clone(), operations_store.clone());
        let sync_config = SyncConfiguration::<AardvarkTopics>::new(sync);

        let network = NetworkBuilder::new(network_id)
            .private_key(private_key.clone())
            .sync(sync_config)
            .build()
            .await
            .expect("network spawning");

        Node::new(
            private_key,
            network,
            operations_store,
            documents_store,
            to_app_tx,
        )
    }

    #[tokio::test]
    async fn discover() {
        let network_id = Hash::new(b"aardvark <3");
        let (to_app_tx_a, _) = mpsc::channel(512);
        let (to_app_tx_b, _) = mpsc::channel(512);

        let mut node_a = test_node(network_id.into(), to_app_tx_a).await;
        let mut node_b = test_node(network_id.into(), to_app_tx_b).await;

        let node_a_addr = node_a.network.endpoint().node_addr().await.unwrap();
        let node_b_addr = node_b.network.endpoint().node_addr().await.unwrap();

        node_a.network.add_peer(node_b_addr).await.unwrap();
        node_b.network.add_peer(node_a_addr).await.unwrap();

        let document = init_document(&mut node_a.operations_store, &node_a.private_key)
            .await
            .expect("can init document");

        let _ = node_a.subscribe(&document).await.unwrap();

        node_a.announce(&document).await.unwrap();

        let discovered_document = node_b.discover(document.short_code()).await.unwrap();

        assert_eq!(discovered_document, document);
    }

    #[tokio::test]
    async fn subscribe_and_unsubscribe() {
        let network_id = Hash::new(b"aardvark <3");
        let (to_app_tx_a, _) = mpsc::channel(512);
        let mut node_a = test_node(network_id.into(), to_app_tx_a).await;

        let document = init_document(&mut node_a.operations_store, &node_a.private_key)
            .await
            .expect("can init document");

        node_a.subscribe(&document).await.unwrap();

        let join_handle = node_a.state.join_handle.unwrap();
        assert!(!join_handle.is_finished());

        node_a.state.unsubscribe_tx.unwrap().send(()).unwrap();
        let result = join_handle.await.unwrap();

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn e2e() {
        let network_id = Hash::new(b"aardvark <3");
        let (to_app_tx_a, _to_app_rx_a) = mpsc::channel(512);
        let (to_app_tx_b, mut to_app_rx_b) = mpsc::channel(512);

        let mut node_a = test_node(network_id.into(), to_app_tx_a).await;
        let node_b = test_node(network_id.into(), to_app_tx_b).await;

        let node_a_addr = node_a.network.endpoint().node_addr().await.unwrap();
        let node_b_addr = node_b.network.endpoint().node_addr().await.unwrap();

        node_a.network.add_peer(node_b_addr).await.unwrap();
        node_b.network.add_peer(node_a_addr).await.unwrap();

        let (from_app_tx_a, from_app_rx_a) = mpsc::channel::<FromApp>(512);
        let (from_app_tx_b, from_app_rx_b) = mpsc::channel::<FromApp>(512);

        let document_a = init_document(&mut node_a.operations_store, &node_a.private_key)
            .await
            .expect("can init document");

        node_a
            .discovered_documents
            .insert(document_a.short_code(), document_a.clone());

        node_a.subscribe(&document_a).await.unwrap();
        node_a.announce(&document_a).await.unwrap();

        node_a.run(from_app_rx_a).await;
        node_b.run(from_app_rx_b).await;

        from_app_tx_b
            .send(FromApp::SubscribeToDocument(document_a.short_code()))
            .await
            .unwrap();

        let ToApp::SubscriptionSuccess(document_a_again) = to_app_rx_b.recv().await.unwrap() else {
            panic!("expected new document enum variant")
        };

        assert_eq!(document_a, document_a_again);

        from_app_tx_a
            .send(FromApp::HandleMessage(vec![0, 1, 2, 3]))
            .await
            .unwrap();

        let ToApp::MessageReceived(bytes) = to_app_rx_b.recv().await.unwrap() else {
            panic!("expected message enum variant")
        };

        assert_eq!(bytes, vec![0, 1, 2, 3])
    }
}
