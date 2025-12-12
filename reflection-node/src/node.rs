use std::path::Path;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use p2panda_core::{Hash, PrivateKey};
use p2panda_net::TopicId;
use thiserror::Error;

use crate::node_inner::NodeInner;
use crate::topic::{SubscribableTopic, Subscription, TopicError};
pub use crate::topic_store::Author;
use crate::topic_store::StoreTopic;

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
pub struct Topic<ID> {
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

    pub async fn topics<ID: From<[u8; 32]>>(&self) -> Result<Vec<Topic<ID>>, TopicError> {
        let inner_clone = self.inner.clone();
        let topics = self
            .inner
            .runtime
            .spawn(async move { inner_clone.topic_store.topics().await })
            .await??;

        let topics = topics
            .into_iter()
            .map(|topic| {
                let StoreTopic {
                    id,
                    name,
                    last_accessed,
                    authors,
                } = topic;
                Topic {
                    id: id.into(),
                    name,
                    last_accessed,
                    authors,
                }
            })
            .collect();

        Ok(topics)
    }

    pub async fn subscribe<ID: Into<[u8; 32]>, T: SubscribableTopic + 'static>(
        &self,
        id: ID,
        topic_handle: T,
    ) -> Result<Subscription<T>, TopicError> {
        let id: TopicId = id.into();
        let topic_handle = Arc::new(topic_handle);
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move { inner_clone.subscribe(id, topic_handle).await })
            .await?
    }

    pub async fn delete_topic<ID: Into<[u8; 32]>>(&self, id: ID) -> Result<(), TopicError> {
        let id: TopicId = id.into();
        let inner_clone = self.inner.clone();
        self.inner
            .runtime
            .spawn(async move { inner_clone.delete_topic(id).await })
            .await?
    }
}
