use std::sync::Arc;

use crate::operation::ReflectionExtensions;
use crate::operation_store::CreationError;
use crate::subscription_inner::SubscriptionInner;

use p2panda_core::{Operation, PublicKey};
use p2panda_sync::protocols::TopicLogSyncEvent;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinError};
use tracing::info;

pub type SyncHandleError = p2panda_net::sync::SyncHandleError<
    Operation<ReflectionExtensions>,
    TopicLogSyncEvent<ReflectionExtensions>,
>;

#[derive(Debug, Error)]
pub enum TopicError {
    #[error(transparent)]
    TopicStore(#[from] sqlx::Error),
    #[error(transparent)]
    OperationStore(#[from] CreationError),
    #[error(transparent)]
    Encode(#[from] p2panda_core::cbor::EncodeError),
    #[error(transparent)]
    Publish(#[from] SyncHandleError),
    #[error(transparent)]
    PublishEphemeral(#[from] mpsc::error::SendError<Vec<u8>>),
    #[error(transparent)]
    Runtime(#[from] JoinError),
}

pub trait SubscribableTopic: Sync + Send {
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

impl<T: SubscribableTopic + 'static> Subscription<T> {
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

    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), TopicError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.send_delta(data).await })
            .await?
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), TopicError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.send_snapshot(data).await })
            .await?
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), TopicError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.send_ephemeral(data).await })
            .await?
    }

    pub async fn unsubscribe(self) -> Result<(), TopicError> {
        let id = self.inner.id;

        self.network_monitor_task.abort();
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.unsubscribe().await })
            .await??;

        info!("Unsubscribed from topic {}", hex::encode(id));

        Ok(())
    }

    /// Set the name for a given topic
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), TopicError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.set_name(name).await })
            .await?
    }
}
