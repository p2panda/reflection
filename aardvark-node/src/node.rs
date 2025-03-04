use std::sync::Arc;

use anyhow::Result;
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::SyncConfiguration;
use p2panda_sync::log_sync::LogSyncProtocol;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use tracing::warn;

use crate::document::Document;
use crate::network::Network;
use crate::operation::{create_operation, validate_document_operation};
use crate::store::{DocumentStore, OperationStore};

pub type NodeSender = mpsc::Sender<Vec<u8>>;

pub type NodeReceiver = mpsc::Receiver<Vec<u8>>;

pub struct Node {
    inner: Arc<NodeInner>,
}

impl Default for Node {
    fn default() -> Self {
        Node::new()
    }
}

struct NodeInner {
    runtime: Runtime,
    operation_store: OperationStore,
    document_store: DocumentStore,
    network: OnceCell<Network>,
    private_key: OnceCell<PrivateKey>,
}

impl Node {
    pub fn new() -> Self {
        // FIXME: Stores are currently in-memory and do not persist data on the file-system.
        // Related issue: https://github.com/p2panda/aardvark/issues/31
        let operation_store = OperationStore::new();
        let document_store = DocumentStore::new();

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("single-threaded tokio runtime");

        Self {
            inner: Arc::new(NodeInner {
                runtime,
                operation_store,
                document_store,
                network: OnceCell::new(),
                private_key: OnceCell::new(),
            }),
        }
    }

    pub fn run(&self, private_key: PrivateKey, network_id: Hash) {
        let sync_config = {
            let sync = LogSyncProtocol::new(
                self.inner.document_store.clone(),
                self.inner.operation_store.clone(),
            );
            SyncConfiguration::<Document>::new(sync)
        };

        let operation_store = self.inner.operation_store.clone();
        let inner = self.inner.clone();

        self.inner.runtime.spawn(async move {
            inner
                .private_key
                .set(private_key.clone())
                .expect("network can be run only once");

            inner
                .network
                .set(
                    Network::spawn(network_id, private_key, sync_config, operation_store)
                        .await
                        .expect("networking backend"),
                )
                .expect("network can be run only once");
        });
    }

    pub fn shutdown(&self) {
        let network = self.inner.network.get().expect("network running").clone();
        self.inner.runtime.block_on(async move {
            network.shutdown().await.expect("network to shutdown");
        });
    }

    pub fn create_document(&self) -> Result<(Hash, NodeSender, NodeReceiver)> {
        let private_key = self.inner.private_key.get().expect("private key");

        let mut operation_store = self.inner.operation_store.clone();
        let operation = self.inner.runtime.block_on(async {
            create_operation(&mut operation_store, private_key, None, None, false).await
        })?;

        let document: Document = operation
            .header
            .extension()
            .expect("document id from our own logs");
        let document_id = (&document).into();

        // Add ourselves as an author to the document store.
        self.inner.runtime.block_on(async {
            self.inner
                .document_store
                .add_author(document, private_key.public_key())
                .await
        })?;

        let (tx, rx) = self.subscribe(document)?;

        Ok((document_id, tx, rx))
    }

    pub fn join_document(&self, document_id: Hash) -> Result<(NodeSender, NodeReceiver)> {
        let document = document_id.into();
        let (tx, rx) = self.subscribe(document)?;
        Ok((tx, rx))
    }

    fn subscribe(&self, document: Document) -> Result<(NodeSender, NodeReceiver)> {
        let (to_network, mut from_app) = mpsc::channel::<Vec<u8>>(512);
        let (to_app, from_network) = mpsc::channel(512);

        let private_key = self.inner.private_key.get().expect("private key").clone();

        // Add ourselves as an author to the document store.
        self.inner.runtime.block_on(async {
            self.inner
                .document_store
                .add_author(document, private_key.public_key())
                .await
        })?;

        let inner = self.inner.clone();
        let _result: JoinHandle<Result<()>> = self.inner.runtime.spawn(async move {
            let network = inner
                .network
                // Allow concurrent calls by awaiting network instance as it might be still
                // in process of initialisation.
                .get_or_init(|| async {
                    unreachable!("network was initialised in `run` method");
                })
                .await;

            let (document_tx, mut document_rx) = network.subscribe(document).await?;

            // Process the operations and forward application messages to app layer. This is where
            // we "materialize" our application state from incoming "application events".
            let document_store = inner.document_store.clone();
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                while let Some(operation) = document_rx.recv().await {
                    // Validation for our custom "document" extension.
                    if let Err(err) = validate_document_operation(&operation, &document) {
                        warn!(
                            public_key = %operation.header.public_key,
                            seq_num = %operation.header.seq_num,
                            "{err}"
                        );
                        continue;
                    }

                    // When we discover a new author we need to add them to our document store.
                    document_store
                        .add_author(document, operation.header.public_key)
                        .await?;

                    // Forward the payload up to the app.
                    if let Some(body) = operation.body {
                        to_app.send(body.to_bytes()).await?;
                    }
                }

                Ok(())
            });

            // Task for handling events coming from the application layer.
            let mut operation_store = inner.operation_store.clone();
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                while let Some(bytes) = from_app.recv().await {
                    // TODO: set prune flag from the frontend.
                    let prune_flag = false;

                    // Create the p2panda operations with application message as payload.
                    let operation = create_operation(
                        &mut operation_store,
                        &private_key,
                        Some(document),
                        Some(&bytes),
                        prune_flag,
                    )
                    .await?;

                    // Broadcast operation on gossip overlay.
                    document_tx.send(operation).await?;
                }

                Ok(())
            });

            Ok(())
        });

        Ok((to_network, from_network))
    }
}
