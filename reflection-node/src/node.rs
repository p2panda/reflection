use std::path::Path;
use std::sync::{Arc, OnceLock};

use anyhow::Result;
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::SyncConfiguration;
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use p2panda_sync::log_sync::LogSyncProtocol;
use sqlx::{migrate::Migrator, sqlite};
use tokio::runtime::Builder;
use tokio::sync::Semaphore;
use tracing::info;

use crate::document::{Document, DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::node_inner::NodeInner;
use crate::operation::LogType;
use crate::operation_store::OperationStore;
use crate::store::DocumentStore;
use crate::utils::CombinedMigrationSource;

pub struct Node {
    inner: OnceLock<Arc<NodeInner>>,
    wait_for_inner: Arc<Semaphore>,
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

    pub async fn subscribe<T: SubscribableDocument + 'static>(
        &self,
        document_id: DocumentId,
        document_handle: T,
    ) -> Result<Subscription, DocumentError> {
        let document_handle = Arc::new(document_handle);
        let inner = self.inner().await;
        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move { inner_clone.subscribe(document_id, document_handle).await })
            .await?
    }
}
