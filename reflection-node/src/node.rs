use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use p2panda_core::{Hash, PrivateKey};
use thiserror::Error;

use crate::document::{DocumentError, DocumentId, SubscribableDocument, Subscription};
pub use crate::document_store::Author;
use crate::document_store::StoreDocument;
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

#[derive(Clone, Debug)]
pub struct Document<ID> {
    pub id: ID,
    pub name: Option<String>,
    pub last_accessed: Option<DateTime<Utc>>,
    pub authors: Vec<Author>,
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

    pub async fn documents<ID: From<[u8; 32]>>(&self) -> Result<Vec<Document<ID>>, DocumentError> {
        let inner_clone = self.inner.clone();
        let documents = self
            .inner
            .runtime
            .spawn(async move { inner_clone.document_store.documents().await })
            .await??;

        let documents = documents
            .into_iter()
            .map(|document| {
                let StoreDocument {
                    id,
                    name,
                    last_accessed,
                    authors,
                } = document;
                Document {
                    id: <[u8; 32]>::from(id).into(),
                    name,
                    last_accessed,
                    authors,
                }
            })
            .collect();

        Ok(documents)
    }

    pub async fn subscribe<ID: Into<[u8; 32]>, T: SubscribableDocument + 'static>(
        &self,
        document_id: ID,
        document_handle: T,
    ) -> Result<Subscription<T>, DocumentError> {
        let document_id: DocumentId = DocumentId::from(document_id.into());
        let document_handle = Arc::new(document_handle);
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move { inner_clone.subscribe(document_id, document_handle).await })
            .await?
    }
}
