use std::sync::Arc;

use crate::node_inner::NodeInner;
use crate::operation::ReflectionExtensions;
use crate::operation_store::CreationError;
use crate::subscription_inner::SubscriptionInner;

use p2panda_core::{Operation, PublicKey};
use p2panda_net::{TopicId, streams::StreamError};
use thiserror::Error;
use tokio::task::{AbortHandle, JoinError};
use tracing::info;

impl From<StreamError<Operation<ReflectionExtensions>>> for DocumentError {
    fn from(value: StreamError<Operation<ReflectionExtensions>>) -> Self {
        DocumentError::Publish(Box::new(value))
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
    // FIXME: The error is huge so but it into a Box
    Publish(Box<StreamError<Operation<ReflectionExtensions>>>),
    #[error(transparent)]
    PublishEphemeral(#[from] StreamError<Vec<u8>>),
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
    network_monitor_task: AbortHandle,
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        self.network_monitor_task.abort();
    }
}

impl<T: SubscribableDocument + 'static> Subscription<T> {
    pub(crate) async fn new(node: Arc<NodeInner>, id: TopicId, document: Arc<T>) -> Self {
        let inner = SubscriptionInner::new(node, id, document);

        let inner_clone = inner.clone();
        let network_monitor_task = inner
            .node
            .runtime
            .spawn(async move {
                inner_clone.spawn_network_monitor().await;
            })
            .abort_handle();

        info!("Subscribed to document {}", hex::encode(id));

        Subscription {
            inner,
            network_monitor_task,
        }
    }

    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.send_delta(data).await })
            .await?
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.send_snapshot(data).await })
            .await?
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.send_ephemeral(data).await })
            .await?
    }

    pub async fn unsubscribe(self) -> Result<(), DocumentError> {
        let document_id = self.inner.id;

        self.network_monitor_task.abort();
        let inner = self.inner.clone();
        inner
            .node
            .clone()
            .runtime
            .spawn(async move { inner.unsubscribe().await })
            .await??;

        info!("Unsubscribed from document {}", hex::encode(document_id));

        Ok(())
    }

    /// Set the name for a given document
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.set_name(name).await })
            .await?
    }
}
