use std::fmt;
use std::hash::Hash;
use std::sync::Arc;

use crate::operation_store::CreationError;
use crate::subscription_inner::SubscriptionInner;

use p2panda_core::PublicKey;
use p2panda_net::{ToNetwork, TopicId};
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    sync::mpsc,
    task::{AbortHandle, JoinError},
};
use tracing::info;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct DocumentId(#[serde(with = "serde_bytes")] [u8; 32]);

impl DocumentId {
    pub const fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl TopicQuery for DocumentId {}

impl TopicId for DocumentId {
    fn id(&self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for DocumentId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<DocumentId> for [u8; 32] {
    fn from(id: DocumentId) -> Self {
        id.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error(transparent)]
    DocumentStore(#[from] sqlx::Error),
    #[error(transparent)]
    OperationStore(#[from] CreationError),
    #[error(transparent)]
    Encode(#[from] p2panda_core::cbor::EncodeError),
    #[error(transparent)]
    Send(#[from] mpsc::error::SendError<ToNetwork>),
    #[error(transparent)]
    Runtime(#[from] JoinError),
}

pub trait SubscribableDocument: Sync + Send {
    fn bytes_received(&self, author: PublicKey, data: Vec<u8>);
    fn author_joined(&self, author: PublicKey);
    fn author_left(&self, author: PublicKey);
    fn ephemeral_bytes_received(&self, author: PublicKey, data: Vec<u8>);
}

pub struct Subscription<T> {
    pub(crate) inner: Arc<SubscriptionInner<T>>,
    pub(crate) runtime: tokio::runtime::Handle,
    network_monitor_task: AbortHandle,
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        self.network_monitor_task.abort();
    }
}

impl<T: SubscribableDocument + 'static> Subscription<T> {
    pub(crate) async fn new(runtime: tokio::runtime::Handle, inner: SubscriptionInner<T>) -> Self {
        let inner = Arc::new(inner);

        let inner_clone = inner.clone();
        let network_monitor_task = runtime
            .spawn(async move {
                inner_clone.spawn_network_monitor().await;
            })
            .abort_handle();

        Subscription {
            inner,
            runtime,
            network_monitor_task,
        }
    }

    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.send_delta(data).await })
            .await?
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.send_snapshot(data).await })
            .await?
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.send_ephemeral(data).await })
            .await?
    }

    pub async fn unsubscribe(self) -> Result<(), DocumentError> {
        let document_id = self.inner.id;

        self.network_monitor_task.abort();
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.unsubscribe().await })
            .await??;

        info!("Unsubscribed from document {}", document_id);

        Ok(())
    }

    /// Set the name for a given document
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.set_name(name).await })
            .await?
    }
}
