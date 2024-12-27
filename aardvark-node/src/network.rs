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
    Subscribe(ShortCode),
    Message(Vec<u8>),
}

pub enum ToApp {
    NewDocument(TextDocument),
    Message(Vec<u8>),
}

pub fn run() -> Result<(
    oneshot::Sender<()>,
    mpsc::Sender<FromApp>,
    mpsc::Receiver<ToApp>,
)> {
    let (to_network, mut from_app) = mpsc::channel::<FromApp>(512);
    let (to_app, from_network) = mpsc::channel(512);

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

            let mut operations_store = MemoryStore::<LogId, AardvarkExtensions>::new();

            let document = init_document(&mut operations_store, &private_key)
                .await
                .expect("can init document");

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

            let mut node = Node::new(private_key, network, operations_store, documents_store);

            to_app
                .send(ToApp::NewDocument(document.clone()))
                .await
                .expect("can send on app channel");

            let _join_handle: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                let (mut topic_tx, mut unsubscribe_tx, mut subscribe_task_handle) = node
                    .subscribe_to_document(&document, to_app.clone())
                    .await?;

                node.announce_document(&document).await?;

                while let Some(message) = from_app.recv().await {
                    match message {
                        FromApp::Subscribe(short_code) => {
                            unsubscribe_tx.send(()).unwrap(); //@TODO: error handling
                            let task_result = subscribe_task_handle.await?;
                            task_result?;

                            let document = node.discover_document(short_code).await?;
                            (topic_tx, unsubscribe_tx, subscribe_task_handle) = node
                                .subscribe_to_document(&document, to_app.clone())
                                .await?;

                            node.announce_document(&document).await?;
                        }
                        FromApp::Message(bytes) => {
                            node.handle_application_bytes(bytes, &document, &mut topic_tx)
                                .await?
                        }
                    }
                }

                Ok(())
            });

            shutdown_rx.await.unwrap();
        });
    });

    Ok((shutdown_tx, to_network, from_network))
}

/// Struct encapsulating core application logic such as; discovering and announcing documents,
/// subscribing and unsubscribing to documents, ingesting application message bytes.
pub struct Node {
    private_key: PrivateKey,
    network: Network<AardvarkTopics>,
    operations_store: MemoryStore<LogId, AardvarkExtensions>,
    documents_store: TextDocumentStore,
}

impl Node {
    pub fn new(
        private_key: PrivateKey,
        network: Network<AardvarkTopics>,
        operations_store: MemoryStore<LogId, AardvarkExtensions>,
        documents_store: TextDocumentStore,
    ) -> Self {
        Node {
            private_key,
            network,
            operations_store,
            documents_store,
        }
    }

    /// Handle application message bytes for a document.
    pub async fn handle_application_bytes(
        &mut self,
        bytes: Vec<u8>,
        document: &TextDocument,
        topic_tx: &mut mpsc::Sender<ToNetwork>,
    ) -> Result<()> {
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
        topic_tx
            .send(ToNetwork::Message {
                bytes: encoded_gossip_operation,
            })
            .await?;
        Ok(())
    }

    /// Discover a document based on its `ShortCode`.
    ///
    /// Note: this method returns a future which may never resolve if no peers in our network are
    /// announcing his document.
    pub async fn discover_document(&mut self, short_code: ShortCode) -> Result<TextDocument> {
        let (_, mut topic_rx, _) = self
            .network
            .subscribe(AardvarkTopics::DiscoveryCode(DiscoveryCode(short_code)))
            .await?;

        if let FromNetwork::GossipMessage { bytes, .. } =
            topic_rx.recv().await.expect("channel to be open")
        {
            let document: TextDocument = cbor::decode_cbor(&bytes[..])?;
            Ok(document)
        } else {
            Err(anyhow::format_err!(
                "Unexpected message received via sync on discovery channel"
            ))
        }
    }

    /// Spawn a task which announces documents on a discovery gossip overlay.
    pub async fn announce_document(&self, document: &TextDocument) -> Result<()> {
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
    pub async fn subscribe_to_document(
        &mut self,
        document: &TextDocument,
        to_app: mpsc::Sender<ToApp>,
    ) -> Result<(
        mpsc::Sender<ToNetwork>, // topic channel
        oneshot::Sender<()>,     // oneshot unsubscribe channel
        JoinHandle<Result<()>>,  // task join handle
    )> {
        let (topic_tx, topic_rx, ready) = self
            .network
            .subscribe(AardvarkTopics::TextDocument(document.clone()))
            .await?;

        // Insert any document we subscribe to into the document store so that sync sessions can
        // correctly occur.
        self.documents_store
            .write()
            .authors
            .insert(self.private_key.public_key(), vec![document.clone()]);

        let (unsubscribe_tx, unsubscribe_rx) = oneshot::channel();
        let unsubscribe_rx = unsubscribe_rx.shared();

        let document_clone = document.clone();
        tokio::task::spawn(async move {
            let _ = ready.await;
            println!("network joined for document: {document_clone:?}");
        });

        let document = document.clone();
        let documents_store = self.documents_store.clone();
        let operations_store = self.operations_store.clone();

        // Task for handling operations arriving from the network.
        let join_handle: JoinHandle<Result<()>> = tokio::task::spawn(async move {
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
                .ingest(operations_store, 128);

            // Process the operations and forward application messages to app layer.
            loop {
                let unsubscribe_rx_clone = unsubscribe_rx.clone();
                select! {
                    _ = unsubscribe_rx_clone => {
                        return Ok(())
                    }
                    Some(message) = stream.next() => {
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
                                            if !documents.contains(&document) {
                                                documents.push(document.clone());
                                            }
                                        })
                                        .or_insert(vec![document.clone()]);
                                };

                                // Forward the payload up to the app.
                                to_app
                                    .send(ToApp::Message(
                                        operation
                                            .body
                                            .expect("all operations have a body")
                                            .to_bytes(),
                                    ))
                                    .await?;
                            }
                            Err(err) => {
                                eprintln!("could not ingest message: {err}");
                            }
                        }
                    }
                }
            }
        });

        Ok((topic_tx, unsubscribe_tx, join_handle))
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
    use crate::network::{LogId, Node};
    use crate::operation::{init_document, AardvarkExtensions};
    use crate::topics::AardvarkTopics;

    async fn test_node(network_id: NetworkId) -> Node {
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

        Node::new(private_key, network, operations_store, documents_store)
    }

    #[tokio::test]
    async fn discover_document() {
        let network_id = Hash::new(b"aardvark <3");
        let mut node_a = test_node(network_id.into()).await;
        let mut node_b = test_node(network_id.into()).await;

        let node_a_addr = node_a.network.endpoint().node_addr().await.unwrap();
        let node_b_addr = node_b.network.endpoint().node_addr().await.unwrap();

        node_a.network.add_peer(node_b_addr).await.unwrap();
        node_b.network.add_peer(node_a_addr).await.unwrap();

        let document = init_document(&mut node_a.operations_store, &node_a.private_key)
            .await
            .expect("can init document");

        let (to_app, _) = mpsc::channel(512);

        let _ = node_a
            .subscribe_to_document(&document, to_app)
            .await
            .unwrap();

        node_a.announce_document(&document).await.unwrap();

        let discovered_document = node_b
            .discover_document(document.short_code())
            .await
            .unwrap();

        assert_eq!(discovered_document, document);
    }

    #[tokio::test]
    async fn subscribe_and_unsubscribe() {
        let network_id = Hash::new(b"aardvark <3");
        let mut node_a = test_node(network_id.into()).await;

        let document = init_document(&mut node_a.operations_store, &node_a.private_key)
            .await
            .expect("can init document");

        let (to_app, _) = mpsc::channel(512);

        let (_, unsubscribe_tx, join_handle) = node_a
            .subscribe_to_document(&document, to_app)
            .await
            .unwrap();

        unsubscribe_tx.send(()).unwrap();
        let result = join_handle.await.unwrap();
        assert!(result.is_ok());
    }
}
