use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use chrono::Utc;
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::SyncConfiguration;
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use p2panda_sync::log_sync::LogSyncProtocol;
use sqlx::{migrate::Migrator, sqlite};
use tokio::runtime::Builder;
use tokio::sync::{RwLock, Semaphore};
use tracing::info;

use crate::document::{Document, DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::ephemerial_operation::EphemerialOperation;
use crate::node_inner::NodeInner;
use crate::operation::LogType;
use crate::operation_store::OperationStore;
use crate::store::DocumentStore;
use crate::utils::CombinedMigrationSource;

pub struct Node {
    inner: OnceLock<Arc<NodeInner>>,
    wait_for_inner: Arc<Semaphore>,
    documents: Arc<RwLock<HashMap<DocumentId, Arc<dyn SubscribableDocument>>>>,
}

impl Default for Node {
    fn default() -> Self {
        Node::new()
    }
}

impl Node {
    pub fn new() -> Self {
        Self {
            inner: OnceLock::new(),
            wait_for_inner: Arc::new(Semaphore::new(0)),
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn inner(&self) -> &Arc<NodeInner> {
        if !self.wait_for_inner.is_closed() {
            // We don't care whether we fail to acquire a permit,
            // once the semaphore is closed `NodeInner` exsists
            let _permit = self.wait_for_inner.acquire().await;
            self.wait_for_inner.close();
        }

        self.inner
            .get()
            .expect("Inner should always be set at this point")
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
            let sync = LogSyncProtocol::new(document_store.clone(), operation_store.clone_inner());
            SyncConfiguration::<DocumentId>::new(sync)
        };

        let inner = Arc::new(
            NodeInner::new(
                runtime,
                network_id,
                private_key,
                sync_config,
                operation_store,
                document_store,
            )
            .await?,
        );

        self.inner.set(inner).expect("Node can be run only once");
        self.wait_for_inner.add_permits(1);

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<()> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move { inner_clone.shutdown().await })
            .await??;

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

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move {
                let operation = inner_clone
                    .operation_store
                    .create_operation(
                        &inner_clone.private_key,
                        LogType::Snapshot,
                        None,
                        None,
                        false,
                    )
                    .await?;

                let document_id: DocumentId = operation
                    .header
                    .extension()
                    .expect("document id from our own logs");
                inner_clone
                    .document_store
                    .add_document(&document_id)
                    .await?;

                // Add ourselves as an author to the document store.
                inner_clone
                    .document_store
                    .add_author(&document_id, &inner_clone.private_key.public_key())
                    .await?;
                Ok(document_id)
            })
            .await?
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

    pub async fn subscribe<T: SubscribableDocument + 'static>(
        &self,
        document_id: DocumentId,
        document_handle: T,
    ) -> Result<Subscription, DocumentError> {
        let document_handle = Arc::new(document_handle);

        self.documents
            .write()
            .await
            .insert(document_id, document_handle.clone());

        let inner = self.inner().await;
        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move { inner_clone.subscribe(document_id, document_handle).await })
            .await?
    }

    pub async fn unsubscribe(&self, document_id: &DocumentId) -> Result<()> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        let document_id = *document_id;

        inner
            .runtime
            .spawn(async move {
                inner_clone
                    .document_store
                    .set_last_accessed_for_document(&document_id, Some(Utc::now()))
                    .await?;

                let result = inner_clone.unsubscribe(&document_id).await;
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

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move {
                // Append one operation to our "ephemeral" delta log.
                let operation = inner_clone
                    .operation_store
                    .create_operation(
                        &inner_clone.private_key,
                        LogType::Delta,
                        Some(document_id),
                        Some(&bytes),
                        false,
                    )
                    .await?;

                // Broadcast operation on gossip overlay.
                inner_clone.send_operation(&document_id, operation).await
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

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move {
                // Append an operation to our "snapshot" log and set the prune flag to
                // true. This will remove previous snapshots.
                //
                // Snapshots are not broadcasted on the gossip overlay as they would be
                // too large. Peers will sync them up when they join the document.
                inner_clone
                    .operation_store
                    .create_operation(
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
                let operation = inner_clone
                    .operation_store
                    .create_operation(
                        &inner_clone.private_key,
                        LogType::Delta,
                        Some(document_id),
                        None,
                        true,
                    )
                    .await?;

                // Broadcast operation on gossip overlay.
                inner_clone.send_operation(&document_id, operation).await
            })
            .await??;

        info!("Snapshot saved for document with id {}", document_id);

        Ok(())
    }

    pub async fn ephemeral(&self, document_id: DocumentId, data: Vec<u8>) -> Result<()> {
        let inner = self.inner().await;

        let operation = EphemerialOperation::new(data, &inner.private_key);
        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move {
                // Broadcast ephemeral data on gossip overlay.
                inner_clone.send_ephemeral(&document_id, operation).await
            })
            .await??;

        info!("Ephemeral data send for document with id {}", document_id);

        Ok(())
    }
}
