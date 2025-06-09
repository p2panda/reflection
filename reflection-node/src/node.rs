use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use chrono::Utc;
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::{SyncConfiguration, SystemEvent};
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use p2panda_sync::log_sync::LogSyncProtocol;
use sqlx::{migrate::Migrator, sqlite};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::{Notify, RwLock, Semaphore};
use tracing::{error, info, warn};

use crate::document::{Document, DocumentId, SubscribableDocument};
use crate::network::Network;
use crate::operation::{LogType, create_operation, validate_operation};
use crate::store::{DocumentStore, OperationStore};
use crate::utils::CombinedMigrationSource;

pub struct Node {
    inner: OnceLock<Arc<NodeInner>>,
    ready_notify: Arc<Notify>,
    documents: Arc<RwLock<HashMap<DocumentId, Arc<dyn SubscribableDocument>>>>,
    semaphore_operation_store: Semaphore,
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
            documents: Arc::new(RwLock::new(HashMap::new())),
            // FIXME: This makes sure we only create one operation at the time and not in parallel
            // Since we would mess up the sequence of operations
            semaphore_operation_store: Semaphore::new(1),
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
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        let _guard = runtime.enter();

        let connection_options = sqlx::sqlite::SqliteConnectOptions::new()
            .shared_cache(true)
            .create_if_missing(true);
        let connection_options = if let Some(db_location) = db_location {
            let db_file = db_location.join("database.sqlite");
            info!("Database file location: {db_file:?}");
            connection_options.filename(db_file)
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

        // Run migration for p2panda OperationStore and for the our DocumentStore
        Migrator::new(CombinedMigrationSource::new(vec![
            operation_store_migrations(),
            sqlx::migrate!(),
        ]))
        .await?
        .run(&pool)
        .await?;

        let operation_store = OperationStore::new(pool.clone());
        let document_store = DocumentStore::new(pool);

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
        let inner = Arc::new(NodeInner {
            runtime,
            operation_store,
            document_store,
            network,
            private_key,
        });

        let documents = self.documents.clone();

        let inner_clone = inner.clone();
        inner
            .network
            .subscribe_events(move |system_event| {
                let documents = documents.clone();
                let inner_clone = inner_clone.clone();
                async move {
                    match system_event {
                        SystemEvent::GossipJoined { topic_id, peers } => {
                            if let Some(document) = documents.read().await.get(&topic_id.into()) {
                                document.authors_joined(peers);
                            }
                        }
                        SystemEvent::GossipNeighborUp { topic_id, peer } => {
                            if let Some(document) = documents.read().await.get(&topic_id.into()) {
                                document.author_set_online(peer, true);
                            }
                        }
                        SystemEvent::GossipNeighborDown { topic_id, peer } => {
                            if let Err(error) = inner_clone
                                .document_store
                                .set_last_seen_for_author(peer, Some(Utc::now()))
                                .await
                            {
                                error!("Failed to set last seen for author {peer}: {error}");
                            }
                            if let Some(document) = documents.read().await.get(&topic_id.into()) {
                                document.author_set_online(peer, false);
                            }
                        }
                        _ => {}
                    };
                }
            })
            .await?;

        self.inner.set(inner).expect("Node can be run only once");
        self.ready_notify.notify_waiters();

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        let inner = self.inner().await;
        let _guard = inner.runtime.enter();

        inner.network.shutdown().await?;

        Ok(())
    }

    pub async fn documents(&self) -> Result<Vec<Document>> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        Ok(inner
            .runtime
            .spawn(async move { inner_clone.document_store.documents().await })
            .await??)
    }

    pub async fn create_document(&self) -> Result<DocumentId> {
        let inner = self.inner().await;
        let _permit = self.semaphore_operation_store.acquire().await.unwrap();

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

    /// Set the name for a given document
    ///
    /// This information will be written to the database
    pub async fn set_name_for_document(
        &self,
        document_id: &DocumentId,
        name: Option<String>,
    ) -> Result<()> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        let document_id = *document_id;
        inner
            .runtime
            .spawn(async move {
                inner_clone
                    .document_store
                    .set_name_for_document(&document_id, name)
                    .await
            })
            .await??;

        Ok(())
    }

    // TODO: check if peers are online and call SubscribableDocument::author_set_online().
    // This requires system events tracking
    pub async fn subscribe<T: SubscribableDocument + 'static>(
        &self,
        document_id: DocumentId,
        document: T,
    ) -> Result<()> {
        let document = Arc::new(document);
        let inner = self.inner().await;
        let _permit = self.semaphore_operation_store.acquire().await.unwrap();

        let inner_clone = inner.clone();
        let stored_operations = inner
            .runtime
            .spawn(async move {
                inner_clone
                    .document_store
                    .add_document(&document_id)
                    .await?;
                // Add ourselves as an author to the document store.
                inner_clone
                    .document_store
                    .add_author(&document_id, &inner_clone.private_key.public_key())
                    .await?;
                inner_clone
                    .document_store
                    .operations_for_document(&inner_clone.operation_store, &document_id)
                    .await
            })
            .await??;

        for operation in stored_operations {
            // Send all stored operation bytes to the app,
            // it doesn't matter if the app already knows some or all of them
            if let Some(body) = operation.body {
                document.bytes_received(operation.header.public_key, body.to_bytes());
            }
        }

        let inner_clone = inner.clone();
        let document_clone = document.clone();
        inner
            .runtime
            .spawn(async move {
                let inner_clone2 = inner_clone.clone();
                inner_clone2
                    .network
                    .subscribe(document_id, move |operation| {
                        let inner_clone = inner_clone.clone();
                        let document_clone = document_clone.clone();
                        async move {
                            // Process the operations and forward application messages to app layer. This is where
                            // we "materialize" our application state from incoming "application events".
                            // Validation for our custom "document" extension.
                            if let Err(err) = validate_operation(&operation, &document_id) {
                                warn!(
                                    public_key = %operation.header.public_key,
                                    seq_num = %operation.header.seq_num,
                                    "{err}"
                                );
                                return;
                            }

                            // When we discover a new author we need to add them to our document store.
                            if let Err(error) = inner_clone
                                .document_store
                                .add_author(&document_id, &operation.header.public_key)
                                .await
                            {
                                error!("Can't store author to database: {error}");
                            }

                            // Forward the payload up to the app.
                            if let Some(body) = operation.body {
                                document_clone
                                    .bytes_received(operation.header.public_key, body.to_bytes());
                            }
                        }
                    })
                    .await
            })
            .await??;

        self.documents.write().await.insert(document_id, document);

        Ok(())
    }

    pub async fn unsubscribe(&self, document_id: &DocumentId) -> Result<()> {
        let inner = self.inner().await;
        let _permit = self.semaphore_operation_store.acquire().await.unwrap();

        let inner_clone = inner.clone();
        let document_id = *document_id;

        inner
            .runtime
            .spawn(async move {
                inner_clone
                    .document_store
                    .set_last_accessed_for_document(&document_id, Some(Utc::now()))
                    .await?;

                let result = inner_clone.network.unsubscribe(&document_id).await;
                result
            })
            .await??;
        self.documents.write().await.remove(&document_id);

        Ok(())
    }

    /// Broadcast a "text delta" on the gossip overlay.
    ///
    /// This should be used to inform all subscribed peers about small changes to the text
    /// document (Delta-Based CRDT).
    pub async fn delta(&self, document_id: DocumentId, bytes: Vec<u8>) -> Result<()> {
        let inner = self.inner().await;
        let _permit = self.semaphore_operation_store.acquire().await.unwrap();

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

                // Broadcast operation on gossip overlay.
                inner_clone
                    .network
                    .send_operation(&document_id, operation)
                    .await
            })
            .await??;

        info!("Delta operation sent for document with id {}", document_id);

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
    pub async fn snapshot(&self, document_id: DocumentId, snapshot_bytes: Vec<u8>) -> Result<()> {
        let inner = self.inner().await;
        let _permit = self.semaphore_operation_store.acquire().await.unwrap();

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
                    Some(document_id),
                    None,
                    true,
                )
                .await?;

                // Broadcast operation on gossip overlay.
                inner_clone
                    .network
                    .send_operation(&document_id, operation)
                    .await
            })
            .await??;

        info!("Snapshot saved for document with id {}", document_id);

        Ok(())
    }
}
