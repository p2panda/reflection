use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::{SyncConfiguration, SystemEvent, TopicId};
use p2panda_store::sqlite::store::run_pending_migrations;
use p2panda_sync::log_sync::LogSyncProtocol;
use sqlx::sqlite;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Notify;
use tracing::warn;

use crate::document::{DocumentId, SubscribableDocument};
use crate::network::Network;
use crate::operation::{LogType, create_operation, validate_operation};
use crate::store::{DocumentStore, OperationStore};

#[derive(Clone)]
pub struct Node {
    inner: OnceLock<Arc<NodeInner>>,
    ready_notify: Arc<Notify>,
}

impl Default for Node {
    fn default() -> Self {
        Node::new()
    }
}

#[derive(Debug)]
struct NodeInner {
    runtime: Runtime,
    operation_store: OperationStore,
    document_store: DocumentStore,
    network: Network,
    private_key: PrivateKey,
}

impl Node {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
            ready_notify: Arc::new(Notify::new()),
        }
    }

    async fn inner(&self) -> &Arc<NodeInner> {
        if let Some(inner) = self.inner.get() {
            inner
        } else {
            self.ready_notify.notified().await;
            self.inner
                .get()
                .expect("Inner should always be set at this point")
        }
    }

    pub async fn run(
        &self,
        private_key: PrivateKey,
        network_id: Hash,
        db_location: Option<&Path>,
    ) -> Result<()> {
        // FIXME: Stores are currently in-memory and do not persist data on the file-system.
        // Related issue: https://github.com/p2panda/aardvark/issues/3
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        let _guard = runtime.enter();

        let connection_options = sqlx::sqlite::SqliteConnectOptions::new()
            .shared_cache(true)
            .create_if_missing(true);
        let connection_options = if let Some(db_location) = db_location {
            connection_options.filename(db_location)
        } else {
            connection_options.in_memory(true)
        };

        let pool = if db_location.is_some() {
            sqlx::sqlite::SqlitePool::connect_with(connection_options).await?
        } else {
            // FIXME: we need to set max connection to 1 for in memory sqlite DB.
            // Probably has to do something with this issue: https://github.com/launchbadge/sqlx/issues/2510
            let pool_options = sqlite::SqlitePoolOptions::new().max_connections(1);
            pool_options.connect_with(connection_options).await?
        };

        // FIXME: Migrate the OperationStore Sqlite DB, I think p2panda-store should call this internally
        run_pending_migrations(&pool).await?;
        // TODO: Do migration of our DB
        // sqlx::migrate!().run(&pool).await?;

        let operation_store = OperationStore::new(pool);
        let document_store = DocumentStore::new();

        let sync_config = {
            let sync = LogSyncProtocol::new(document_store.clone(), operation_store.clone());
            SyncConfiguration::<DocumentId>::new(sync)
        };

        let network = Network::spawn(
            network_id,
            private_key.clone(),
            sync_config,
            operation_store.clone(),
        )
        .await?;

        self.inner
            .set(Arc::new(NodeInner {
                runtime,
                operation_store,
                document_store,
                network,
                private_key,
            }))
            .expect("Node can be run only once");
        self.ready_notify.notify_waiters();

        Ok(())
    }

    pub fn shutdown(&self) {
        if let Some(inner) = self.inner.get() {
            let network = inner.network.clone();
            inner.runtime.block_on(async move {
                network.shutdown().await.expect("network to shutdown");
            });
        }
    }

    pub async fn create_document(&self) -> Result<DocumentId> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        let operation = inner.runtime.block_on(async {
            create_operation(
                &mut inner_clone.operation_store.clone(),
                &inner_clone.private_key,
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
        let document = Arc::new(document);
        let inner = self.inner().await;

        // Add ourselves as an author to the document store.
        inner
            .document_store
            .add_author(document_id, inner.private_key.public_key())
            .await?;

        let inner_clone = inner.clone();
        let (document_tx, mut document_rx, mut system_event) = inner
            .runtime
            .spawn(async move { inner_clone.network.subscribe(document_id).await })
            .await
            .unwrap()?;
        inner
            .document_store
            .set_subscription_for_document(document_id, document_tx)
            .await;

        let inner_clone = inner.clone();
        let document_clone = document.clone();
        inner.runtime.spawn(async move {
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
                inner_clone
                    .document_store
                    .add_author(document_id, operation.header.public_key)
                    .await
                    .expect("Unable to add author to DocumentStore");

                // Forward the payload up to the app.
                if let Some(body) = operation.body {
                    document_clone.bytes_received(operation.header.public_key, &body.to_bytes());
                }
            }
        });

        inner.runtime.spawn(async move {
            while let Ok(system_event) = system_event.recv().await {
                match system_event {
                    SystemEvent::GossipJoined { topic_id, peers }
                        if topic_id == document_id.id() =>
                    {
                        document.authors_joined(peers);
                    }
                    SystemEvent::GossipNeighborUp { topic_id, peer }
                        if topic_id == document_id.id() =>
                    {
                        document.author_set_online(peer, true);
                    }
                    SystemEvent::GossipNeighborDown { topic_id, peer }
                        if topic_id == document_id.id() =>
                    {
                        document.author_set_online(peer, false);
                    }
                    _ => {}
                };
            }
        });

        Ok(())
    }

    /// Broadcast a "text delta" on the gossip overlay.
    ///
    /// This should be used to inform all subscribed peers about small changes to the text
    /// document (Delta-Based CRDT).
    pub async fn delta(&self, document_id: DocumentId, bytes: Vec<u8>) -> Result<()> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move {
                let mut operation_store = inner_clone.operation_store.clone();
                // Append one operation to our "ephemeral" delta log.
                let operation = create_operation(
                    &mut operation_store,
                    &inner_clone.private_key,
                    LogType::Delta,
                    Some(document_id),
                    Some(&bytes),
                    false,
                )
                .await?;

                let document_tx = inner_clone
                    .document_store
                    .subscription_for_document(document_id)
                    .await
                    .expect("Not subscribed to document");

                // Broadcast operation on gossip overlay.
                document_tx.send(operation).await?;
                Ok(())
            })
            .await?
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
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move {
                let mut operation_store = inner_clone.operation_store.clone();

                // Append an operation to our "snapshot" log and set the prune flag to
                // true. This will remove previous snapshots.
                //
                // Snapshots are not broadcasted on the gossip overlay as they would be
                // too large. Peers will sync them up when they join the document.
                create_operation(
                    &mut operation_store,
                    &inner_clone.private_key,
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
                    &mut operation_store,
                    &inner_clone.private_key,
                    LogType::Delta,
                    Some(document_id.into()),
                    Some(&delta_bytes),
                    true,
                )
                .await?;

                let document_tx = inner_clone
                    .document_store
                    .subscription_for_document(document_id)
                    .await
                    .expect("Not subscribed to document");

                // Broadcast operation on gossip overlay.
                document_tx.send(operation).await?;

                Ok(())
            })
            .await?
    }
}
