use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::TopicId;
use thiserror::Error;
use tracing::info;

use crate::document::{DocumentError, SubscribableDocument, Subscription};
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
enum OwnedRuntimeOrHandle {
    Handle(tokio::runtime::Handle),
    OwnedRuntime(tokio::runtime::Runtime),
}

impl std::ops::Deref for OwnedRuntimeOrHandle {
    type Target = tokio::runtime::Handle;

    fn deref(&self) -> &Self::Target {
        match self {
            OwnedRuntimeOrHandle::Handle(handle) => handle,
            OwnedRuntimeOrHandle::OwnedRuntime(runtime) => runtime.handle(),
        }
    }
}

#[derive(Debug)]
pub struct Node {
    inner: Arc<NodeInner>,
    runtime: OwnedRuntimeOrHandle,
}

impl Node {
    pub async fn new(
        private_key: PrivateKey,
        network_id: Hash,
        db_location: Option<&Path>,
        connection_mode: ConnectionMode,
    ) -> Result<Self, NodeError> {
        let runtime = if let Ok(handle) = tokio::runtime::Handle::try_current() {
            OwnedRuntimeOrHandle::Handle(handle)
        } else {
            OwnedRuntimeOrHandle::OwnedRuntime(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?,
            )
        };

        let db_file = db_location.map(|location| location.join("database.sqlite"));
        let inner = runtime
            .spawn(async move {
                NodeInner::new(network_id, private_key, db_file, connection_mode).await
            })
            .await??;

        Ok(Self {
            inner: Arc::new(inner),
            runtime,
        })
    }

    pub async fn set_connection_mode(
        &self,
        connection_mode: ConnectionMode,
    ) -> Result<(), NodeError> {
        let inner_clone = self.inner.clone();
        self.runtime
            .spawn(async move {
                inner_clone.set_connection_mode(connection_mode).await;
            })
            .await?;

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), NodeError> {
        let inner_clone = self.inner.clone();
        self.runtime
            .spawn(async move {
                inner_clone.shutdown().await;
            })
            .await?;

        Ok(())
    }

    pub async fn documents<ID: From<[u8; 32]>>(&self) -> Result<Vec<Document<ID>>, DocumentError> {
        let inner_clone = self.inner.clone();
        let documents = self
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
                    id: id.into(),
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
        id: ID,
        document_handle: T,
    ) -> Result<Subscription<T>, DocumentError> {
        let id: TopicId = id.into();
        let document_handle = Arc::new(document_handle);
        let inner_clone = self.inner.clone();
        let inner_subscription = self
            .runtime
            .spawn(async move { inner_clone.subscribe(id, document_handle).await })
            .await??;

        let subscription = Subscription::new(self.runtime.clone(), inner_subscription).await;
        info!("Subscribed to topic {}", hex::encode(id));

        Ok(subscription)
    }

    pub async fn delete_document<ID: Into<[u8; 32]>>(&self, id: ID) -> Result<(), DocumentError> {
        let id: TopicId = id.into();
        let inner_clone = self.inner.clone();
        self.runtime
            .spawn(async move { inner_clone.delete_document(id).await })
            .await?
    }
}
