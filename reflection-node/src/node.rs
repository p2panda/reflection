use std::path::Path;
use std::sync::Arc;

use p2panda_core::{Hash, PrivateKey};
use thiserror::Error;

use crate::document::{DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::document_store::Document;
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
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    #[default]
    None,
    Bluetooth,
    Network,
}

#[derive(Debug)]
pub struct Node {
    inner: Arc<NodeInner>,
}

impl Node {
    pub async fn new(
        private_key: PrivateKey,
        network_id: Hash,
        db_location: Option<&Path>,
        connection_mode: ConnectionMode,
    ) -> Result<Self, NodeError> {
        Ok(Self {
            inner: Arc::new(
                NodeInner::new(network_id, private_key, db_location, connection_mode).await?,
            ),
        })
    }

    pub async fn set_connection_mode(
        &self,
        connection_mode: ConnectionMode,
    ) -> Result<(), NodeError> {
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move {
                inner_clone.set_connection_mode(connection_mode).await;
            })
            .await?;

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), NodeError> {
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move {
                inner_clone.shutdown().await;
            })
            .await?;

        Ok(())
    }

    pub async fn documents(&self) -> Result<Vec<Document>, DocumentError> {
        let inner_clone = self.inner.clone();
        Ok(self
            .inner
            .runtime
            .spawn(async move { inner_clone.document_store.documents().await })
            .await??)
    }

    pub async fn create_document(&self) -> Result<DocumentId, DocumentError> {
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move { inner_clone.create_document().await })
            .await?
    }

    pub async fn subscribe<T: SubscribableDocument + 'static>(
        &self,
        document_id: DocumentId,
        document_handle: T,
    ) -> Result<Subscription<T>, DocumentError> {
        let document_handle = Arc::new(document_handle);
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move { inner_clone.subscribe(document_id, document_handle).await })
            .await?
    }
}
