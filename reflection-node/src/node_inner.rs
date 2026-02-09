use std::path::PathBuf;
use std::sync::Arc;

use crate::ephemerial_operation::EphemerialOperation;
use crate::network::Network;
use crate::node::{ConnectionMode, NodeError};
use crate::operation_store::OperationStore;
use crate::subscription_inner::SubscriptionInner;
use crate::topic::{SubscribableTopic, TopicError};
use crate::topic_store::TopicStore;
use crate::utils::CombinedMigrationSource;

use p2panda_core::{Hash, PrivateKey};
use p2panda_net::TopicId;
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use sqlx::{migrate::Migrator, sqlite};
use tokio::sync::{Notify, RwLock};
use tracing::{info, warn};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum MessageType {
    Ephemeral(EphemerialOperation),
    AuthorEphemeral(EphemerialOperation),
}

#[derive(Debug)]
pub struct NodeInner {
    pub(crate) operation_store: OperationStore,
    pub(crate) topic_store: TopicStore,
    pub(crate) private_key: PrivateKey,
    pub(crate) network_id: Hash,
    pub(crate) network: RwLock<Option<Network>>,
    pub(crate) network_notifier: Notify,
}

impl NodeInner {
    pub async fn new(
        network_id: Hash,
        private_key: PrivateKey,
        db_file: Option<PathBuf>,
    ) -> Result<Self, NodeError> {
        let connection_options = sqlx::sqlite::SqliteConnectOptions::new()
            .shared_cache(true)
            .create_if_missing(true);
        let pool = if let Some(db_file) = db_file {
            info!("Database file location: {db_file:?}");
            let connection_options = connection_options.filename(db_file);
            sqlx::sqlite::SqlitePool::connect_with(connection_options).await?
        } else {
            let connection_options = connection_options.in_memory(true);
            // FIXME: we need to set max connection to 1 for in memory sqlite DB.
            // Probably has to do something with this issue: https://github.com/launchbadge/sqlx/issues/2510
            let pool_options = sqlite::SqlitePoolOptions::new().max_connections(1);
            pool_options.connect_with(connection_options).await?
        };

        // Run migration for p2panda OperationStore and for the our TopicStore
        Migrator::new(CombinedMigrationSource::new(vec![
            operation_store_migrations(),
            sqlx::migrate!(),
        ]))
        .await?
        .run(&pool)
        .await?;

        let operation_store = OperationStore::new(pool.clone());
        let topic_store = TopicStore::new(pool);

        Ok(Self {
            operation_store,
            topic_store,
            private_key,
            network_id,
            network: RwLock::new(None),
            network_notifier: Notify::new(),
        })
    }

    pub async fn set_connection_mode(&self, connection_mode: ConnectionMode) {
        // Subscriptions will tear down the network subscription and drop the read lock,
        // so that we can acquire the write lock and then shutdown the network.
        self.network_notifier.notify_waiters();

        let mut network_guard = self.network.write().await;

        let network = match connection_mode {
            ConnectionMode::None => None,
            ConnectionMode::Bluetooth => {
                unimplemented!("Bluetooth is currently not implemented")
            }
            ConnectionMode::Network => {
                match Network::new(
                    &self.private_key,
                    &self.network_id,
                    &self.topic_store,
                    &self.operation_store,
                )
                .await
                {
                    Ok(network) => Some(network),
                    Err(error) => {
                        warn!("Failed to startup network: {error}");
                        None
                    }
                }
            }
        };

        *network_guard = network;
    }

    pub async fn shutdown(&self) {
        // Wake up all subscriptions that may still exist
        self.network_notifier.notify_waiters();
        self.network.write().await.take();
    }

    pub async fn subscribe<T: SubscribableTopic + 'static>(
        self: Arc<Self>,
        id: TopicId,
        subscribable_topic: Arc<T>,
    ) -> Result<SubscriptionInner<T>, TopicError> {
        self.topic_store.add_topic(&id).await?;
        // Add ourselves as an author to the topic store.
        self.topic_store
            .add_author(&id, &self.private_key.public_key())
            .await?;
        let stored_operations = self
            .topic_store
            .operations_for_topic(&self.operation_store, &id)
            .await?;

        for operation in stored_operations {
            // Send all stored operation bytes to the app,
            // it doesn't matter if the app already knows some or all of them
            if let Some(body) = operation.body {
                subscribable_topic.bytes_received(operation.header.public_key, body.to_bytes());
            }
        }

        Ok(SubscriptionInner::new(self.clone(), id, subscribable_topic))
    }

    pub async fn delete_topic(self: Arc<Self>, id: TopicId) -> Result<(), TopicError> {
        self.topic_store.delete_topic(&id).await?;
        Ok(())
    }
}
