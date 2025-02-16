use anyhow::Result;
use p2panda_core::{Hash, PrivateKey, PruneFlag};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::mpsc;
use tokio::sync::OnceCell;
use tokio::task::JoinHandle;
use tracing::debug;

use crate::network::Network;
use crate::operation::create_operation;
use crate::store::{DocumentStore, OperationStore};
use crate::topic::Document;

pub struct Node {
    runtime: Runtime,
    operation_store: OperationStore,
    document_store: DocumentStore,
    network: OnceCell<Network>,
    private_key: OnceCell<PrivateKey>,
}

impl Default for Node {
    fn default() -> Self {
        Node::new()
    }
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
            runtime,
            operation_store,
            document_store,
            network: OnceCell::new(),
            private_key: OnceCell::new(),
        }
    }

    pub fn run(&self, private_key: PrivateKey, network_id: Hash) {
        self.private_key
            .set(private_key.clone())
            .expect("network can be run only once");

        self.runtime.spawn(async move {
            self.network
                .get_or_init(|| async {
                    Network::spawn(
                        network_id,
                        private_key,
                        self.operation_store.clone(),
                        self.document_store.clone(),
                    )
                    .await
                    .expect("networking backend")
                })
                .await;
        });
    }

    pub fn shutdown(&mut self) {
        let network = self.network.take().expect("network running");
        self.runtime.block_on(async move {
            network.shutdown().await.expect("network to shutdown");
        });
    }

    pub fn create_document(
        &self,
    ) -> Result<(Hash, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)> {
        let private_key = self.private_key.get().expect("private key to be set");
        let network = self.network.get().expect("network running");

        let mut operation_store = self.operation_store;

        let operation = self.runtime.block_on(async {
            create_operation(&mut operation_store, private_key, None, None, false).await
        })?;

        let document: Document = operation
            .header
            .extension()
            .expect("document id from our own logs");
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
        document: Document,
    ) -> Result<(mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>)> {
        let (to_network, mut from_app) = mpsc::channel::<Vec<u8>>(512);
        let (to_app, from_network) = mpsc::channel(512);

        let private_key = self.private_key.get().expect("private key to be set");
        let network = self.network.get().expect("network running");

        self.runtime.spawn(async move {
            let (document_tx, document_rx) = network.subscribe(document.clone()).await?;

            let document_store = self.document_store.clone();
            let operation_store = self.operation_store.clone();

            let document_clone = document.clone();
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                // Process the operations and forward application messages to app layer.
                while let Some(operation) = document_rx.recv().await {
                    let prune_flag: PruneFlag = operation.header.extension().unwrap_or_default();
                    debug!(
                        "received operation from {}, seq_num={}, prune_flag={}",
                        operation.header.public_key,
                        operation.header.seq_num,
                        prune_flag.is_set(),
                    );

                    // When we discover a new author we need to add them to our "document store".
                    document_store
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

            // Task for handling events coming from the application layer.
            let _result: JoinHandle<Result<()>> = tokio::task::spawn(async move {
                while let Some(bytes) = from_app.recv().await {
                    // TODO: set prune flag from the frontend.
                    let prune_flag = false;

                    // Create the p2panda operations with application message as payload.
                    let operation = create_operation(
                        &mut operation_store,
                        &private_key,
                        Some(document.clone()),
                        Some(&bytes),
                        prune_flag,
                    )
                    .await?;

                    debug!(
                        "created operation seq_num={}, prune_flag={}, payload_size={}",
                        operation.header.seq_num,
                        prune_flag,
                        bytes.len(),
                    );

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
