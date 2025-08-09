use std::path::Path;
use std::sync::{Arc, OnceLock};

use p2panda_core::{Hash, PrivateKey};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::document::{Document, DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::node_inner::NodeInner;

#[derive(Debug, Error)]
pub enum NodeError {
    #[error(transparent)]
    RuntimeStartup(#[from] std::io::Error),
    #[error(transparent)]
    RuntimeSpawn(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Datebase(#[from] sqlx::Error),
    #[error(transparent)]
    DatebaseMigration(#[from] sqlx::migrate::MigrateError),
    // FIXME: remove anyhow but p2panda uses anyhow
    #[error(transparent)]
    Anyhow(#[from] anyhow::Error),
}

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
    ) -> Result<(), NodeError> {
        let inner = Arc::new(NodeInner::new(network_id, private_key, db_location).await?);

        self.inner.set(inner).expect("Node can be run only once");
        self.wait_for_inner.add_permits(1);

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), NodeError> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move { inner_clone.shutdown().await })
            .await??;

        Ok(())
    }

    pub async fn documents(&self) -> Result<Vec<Document>, DocumentError> {
        let inner = self.inner().await;

        let inner_clone = inner.clone();
        Ok(inner
            .runtime
            .spawn(async move { inner_clone.document_store.documents().await })
            .await??)
    }

    pub async fn create_document(&self) -> Result<DocumentId, DocumentError> {
        let inner = self.inner().await;
        let inner_clone = inner.clone();
        inner
            .runtime
            .spawn(async move { inner_clone.create_document().await })
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
