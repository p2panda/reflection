use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use chrono::{DateTime, Utc};
use p2panda::node::{NetworkId, RelayUrl, SpawnError};
use p2panda_core::{PrivateKey, PublicKey};
use thiserror::Error;
use tokio::sync::{Notify, RwLock};
use tracing::info;

use crate::database::{database_pool, run_migrations};
use crate::subscription::{Subscription, SubscriptionError, SubscriptionInner};
pub use crate::topic_store::Author;
use crate::topic_store::{StoreTopic, TopicStore};
use crate::traits::SubscribableTopic;

static RELAY_URL: LazyLock<RelayUrl> = LazyLock::new(|| {
    "https://euc1-1.relay.n0.iroh-canary.iroh.link"
        .parse()
        .expect("valid relay URL")
});

static BOOTSTRAP_NODE_ID: LazyLock<PublicKey> = LazyLock::new(|| {
    "9f63a15ab95959a992af96bf72fbc3e7dc98eeb4799f788bb07b20125053e795"
        .parse()
        .expect("valid bootstrap node id")
});

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

    #[error(transparent)]
    NodeSpawn(#[from] SpawnError),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Default)]
pub enum ConnectionMode {
    #[default]
    None,
    Bluetooth,
    Network,
}

#[derive(Clone, Debug)]
pub struct Topic {
    pub id: p2panda_core::Topic,
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
        network_id: impl Into<NetworkId>,
        db_location: Option<&Path>,
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

        let inner = {
            let network_id = network_id.into();
            let db_file = db_location.map(|location| location.join("database.sqlite"));

            runtime
                .spawn(NodeInner::new(private_key, network_id, db_file))
                .await??
        };

        Ok(Self {
            inner: Arc::new(inner),
            runtime,
        })
    }

    pub async fn set_connection_mode(
        &self,
        connection_mode: ConnectionMode,
    ) -> Result<(), NodeError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.set_connection_mode(connection_mode).await })
            .await??;

        Ok(())
    }

    pub async fn shutdown(&self) -> Result<(), NodeError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move {
                inner.shutdown().await;
            })
            .await?;

        Ok(())
    }

    pub async fn topics(&self) -> Result<Vec<Topic>, SubscriptionError> {
        let inner = self.inner.clone();
        let topics = self
            .runtime
            .spawn(async move { inner.topic_store.topics().await })
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
                    id,
                    name,
                    last_accessed,
                    authors,
                }
            })
            .collect();

        Ok(topics)
    }

    pub async fn subscribe<T>(
        &self,
        id: impl Into<p2panda_core::Topic>,
        subscribable_topic: T,
    ) -> Result<Subscription<T>, SubscriptionError>
    where
        T: SubscribableTopic + 'static,
    {
        let id = id.into();
        let subscribable_topic = Arc::new(subscribable_topic);
        let inner = self.inner.clone();
        let inner_subscription = self
            .runtime
            .spawn(async move { inner.subscribe(id, subscribable_topic).await })
            .await??;

        let subscription = Subscription::new(self.runtime.clone(), inner_subscription).await;
        info!(%id, "subscribed to topic");

        Ok(subscription)
    }

    pub async fn delete_topic(
        &self,
        id: impl Into<p2panda_core::Topic>,
    ) -> Result<(), SubscriptionError> {
        let id = id.into();
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.delete_topic(id).await })
            .await?
    }
}

#[derive(Debug)]
pub(crate) struct NodeInner {
    pub(crate) network: RwLock<p2panda::Node>,
    pub(crate) shutdown_notifier: Notify,
    pub(crate) topic_store: TopicStore,
    pub(crate) public_key: PublicKey,
}

impl NodeInner {
    pub async fn new(
        private_key: PrivateKey,
        network_id: impl Into<NetworkId>,
        db_file: Option<PathBuf>,
    ) -> Result<Self, NodeError> {
        let public_key = private_key.public_key();

        let pool = database_pool(db_file).await?;
        run_migrations(&pool).await?;

        let topic_store = TopicStore::from_pool(pool.clone());

        let mut builder = p2panda::Node::builder()
            .network_id(network_id.into())
            .private_key(private_key)
            .database_pool(pool);

        // Don't connect to any servers during testing.
        if cfg!(not(test)) {
            builder = builder
                .bootstrap(*BOOTSTRAP_NODE_ID, RELAY_URL.clone())
                .relay_url(RELAY_URL.clone());
        }

        let node = builder.spawn().await?;

        Ok(Self {
            network: RwLock::new(node),
            shutdown_notifier: Notify::new(),
            topic_store,
            public_key,
        })
    }

    pub async fn set_connection_mode(
        &self,
        _connection_mode: ConnectionMode,
    ) -> Result<(), NodeError> {
        // TODO: This is a no-op currently and requires work in `p2panda-net` upstream.
        // See related issue: https://github.com/p2panda/p2panda/issues/1093
        Ok(())
    }

    pub async fn shutdown(&self) {
        // Wake up all subscriptions that may still exist.
        self.shutdown_notifier.notify_waiters();
    }

    pub async fn subscribe<T>(
        self: Arc<Self>,
        id: impl Into<p2panda_core::Topic>,
        subscribable_topic: Arc<T>,
    ) -> Result<SubscriptionInner<T>, SubscriptionError>
    where
        T: SubscribableTopic + 'static,
    {
        let id = id.into();

        self.topic_store.add_topic(&id).await?;

        // Add ourselves as an author to the topic store.
        self.topic_store.add_author(&id, &self.public_key).await?;

        Ok(SubscriptionInner::new(self.clone(), id, subscribable_topic))
    }

    pub async fn delete_topic(
        self: Arc<Self>,
        id: impl Into<p2panda_core::Topic>,
    ) -> Result<(), SubscriptionError> {
        let id = id.into();
        self.topic_store.delete_topic(&id).await?;
        Ok(())
    }
}
