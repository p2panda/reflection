use anyhow::Result;
use iroh_gossip::proto::Config as GossipConfig;
use p2panda_core::{Extension, Hash, PrivateKey, PruneFlag};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::ToNetwork;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration};
use p2panda_store::MemoryStore;
use p2panda_stream::{DecodeExt, IngestExt};
use p2panda_sync::log_sync::LogSyncProtocol;
use tokio::runtime::Builder;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::document::{ShortCode, TextDocumentStore};
use crate::operation::{
    create_operation, decode_gossip_message, encode_gossip_operation, init_document,
    AardvarkExtensions, LogId,
};
use crate::topics::TextDocument;

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

            to_app
                .send(ToApp::NewDocument(document.clone()))
                .await
                .expect("can send on app channel");

            let documents_store = TextDocumentStore::new();
            documents_store
                .write()
                .authors
                .insert(private_key.public_key(), vec![document.clone()]);

            let sync = LogSyncProtocol::new(documents_store.clone(), operations_store.clone());
            let sync_config = SyncConfiguration::<TextDocument>::new(sync);

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

            let (topic_tx, topic_rx, ready) = network
                .subscribe(document.clone())
                .await
                .expect("subscribe to topic");

            tokio::task::spawn(async move {
                let _ = ready.await;
                println!("network joined!");
            });

            // Task for handling operations arriving from the network.
            let operations_store_clone = operations_store.clone();
            let document_clone = document.clone();
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
                                        if !documents.contains(&document_clone) {
                                            documents.push(document_clone.clone());
                                        }
                                    })
                                    .or_insert(vec![document_clone.clone()]);
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

                Ok(())
            });

            // Task for handling events coming from the application layer.
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                while let Some(message) = from_app.recv().await {
                    match message {
                        FromApp::Subscribe(_short_code) => todo!(),
                        FromApp::Message(bytes) => {
                            // @TODO: set prune flag from the frontend.
                            let prune_flag = false;

                            // Create the p2panda operations with application message as payload.
                            let (header, body) = create_operation(
                                &mut operations_store,
                                &private_key,
                                document.clone(),
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
                    }
                }

                Ok(())
            });

            shutdown_rx.await.unwrap();
        });
    });

    Ok((shutdown_tx, to_network, from_network))
}
