use std::sync::Arc;

use anyhow::Result;
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::SyncConfiguration;
use p2panda_sync::log_sync::LogSyncProtocol;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::OnceCell;
use tracing::warn;

use crate::document::{DocumentId, SubscribableDocument};
use crate::network::Network;
use crate::operation::{LogType, create_operation, validate_operation};
use crate::store::{DocumentStore, OperationStore};

#[derive(Clone)]
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
            SyncConfiguration::<DocumentId>::new(sync)
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
                .get_or_init(|| async {
                    Network::spawn(network_id, private_key, sync_config, operation_store)
                        .await
                        .expect("networking backend")
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

    pub fn create_document(&self) -> Result<DocumentId> {
        let private_key = self.inner.private_key.get().expect("private key");

        let mut operation_store = self.inner.operation_store.clone();
        let operation = self.inner.runtime.block_on(async {
            create_operation(
                &mut operation_store,
                private_key,
                LogType::Snapshot,
                None,
                None,
                false,
            )
            .await
        })?;

        let document_id: DocumentId = operation
            .header
            .extension()
            .expect("document id from our own logs");

        Ok(document_id)
    }

    pub async fn subscribe<T: SubscribableDocument + 'static>(
        &self,
        document_id: DocumentId,
        document: T,
    ) -> Result<()> {
        let private_key = self.inner.private_key.get().expect("private key").clone();

        // Add ourselves as an author to the document store.
        self.inner
            .document_store
            .add_author(document_id, private_key.public_key())
            .await?;

        let inner_clone = self.inner.clone();
        let (document_tx, mut document_rx) = self
            .inner
            .runtime
            .spawn(async move {
                let network = inner_clone
                    .network
                    // Allow concurrent calls by awaiting network instance as it might be still
                    // in process of initialisation.
                    .get_or_init(|| async {
                        unreachable!("network was initialised in `run` method");
                    })
                    .await;
                network.subscribe(document_id).await
            })
            .await
            .unwrap()?;
        self.inner
            .document_store
            .set_subscription_for_document(document_id, document_tx)
            .await;

        let inner = self.inner.clone();
        self.inner.runtime.spawn(async move {
            // Process the operations and forward application messages to app layer. This is where
            // we "materialize" our application state from incoming "application events".
            while let Some(operation) = document_rx.recv().await {
                // Validation for our custom "document" extension.
                if let Err(err) = validate_operation(&operation, &document_id) {
                    warn!(
                        public_key = %operation.header.public_key,
                        seq_num = %operation.header.seq_num,
                        "{err}"
                    );
                    continue;
                }

                // When we discover a new author we need to add them to our document store.
                inner
                    .document_store
                    .add_author(document_id, operation.header.public_key)
                    .await
                    .expect("Unable to add author to DocumentStore");

                // Forward the payload up to the app.
                if let Some(body) = operation.body {
                    document.bytes_received(operation.header.public_key, &body.to_bytes());
                }
            }
        });

        Ok(())
    }

    /// Broadcast a "text delta" on the gossip overlay.
    ///
    /// This should be used to inform all subscribed peers about small changes to the text
    /// document (Delta-Based CRDT).
    pub async fn delta(&self, document_id: DocumentId, bytes: Vec<u8>) -> Result<()> {
        let private_key = self.inner.private_key.get().expect("private key");

        // Append one operation to our "ephemeral" delta log.
        let operation = create_operation(
            &mut self.inner.operation_store.clone(),
            &private_key,
            LogType::Delta,
            Some(document_id),
            Some(&bytes),
            false,
        )
        .await?;

        let document_tx = self
            .inner
            .document_store
            .subscription_for_document(document_id)
            .await
            .expect("Not subscribed to document");

        // Broadcast operation on gossip overlay.
        document_tx.send(operation).await?;

        Ok(())
    }

    /// Same as [`Self::Delta`] next to persisting a whole snapshot and pruning.
    ///
    /// Snapshots contain the whole text document history and are much larger than deltas. This
    /// data will only be sent to newly incoming peers via the sync protocol.
    ///
    /// Since a snapshot contains all data we need to reliably reconcile documents (it is a
    /// State-Based CRDT) this command prunes all our logs and removes past snapshot- and delta
    /// operations.
    pub async fn delta_with_snapshot(
        &self,
        document_id: DocumentId,
        delta_bytes: Vec<u8>,
        snapshot_bytes: Vec<u8>,
    ) -> Result<()> {
        let private_key = self.inner.private_key.get().expect("private key");

        // Append an operation to our "snapshot" log and set the prune flag to
        // true. This will remove previous snapshots.
        //
        // Snapshots are not broadcasted on the gossip overlay as they would be
        // too large. Peers will sync them up when they join the document.
        create_operation(
            &mut self.inner.operation_store.clone(),
            &private_key,
            LogType::Snapshot,
            Some(document_id),
            Some(&snapshot_bytes),
            true,
        )
        .await?;

        // Append an operation to our "ephemeral" delta log and set the prune
        // flag to true.
        //
        // This signals removing all previous "delta" operations now. This is
        // some sort of garbage collection whenever we snapshot. Snapshots
        // already contain all history, there is no need to keep duplicate
        // "delta" data around.
        let operation = create_operation(
            &mut self.inner.operation_store.clone(),
            &private_key,
            LogType::Delta,
            Some(document_id.into()),
            Some(&delta_bytes),
            true,
        )
        .await?;

        let document_tx = self
            .inner
            .document_store
            .subscription_for_document(document_id)
            .await
            .expect("Not subscribed to document");

        // Broadcast operation on gossip overlay.
        document_tx.send(operation).await?;

        Ok(())
    }
}
