use std::path::Path;
use std::sync::{Arc, LazyLock};

use crate::ephemerial_operation::EphemerialOperation;
use crate::node::{ConnectionMode, NodeError};
use crate::operation::ReflectionExtensions;
use crate::operation_store::OperationStore;
use crate::topic::{SubscribableTopic, Subscription, TopicError};
use crate::topic_store::{LogId, TopicStore};
use crate::utils::CombinedMigrationSource;

use p2panda_core::{Hash, PrivateKey};
use p2panda_discovery::address_book::AddressBookStore;
use p2panda_discovery::address_book::memory::MemoryStore as MemoryAddressBook;
use p2panda_net::{MdnsDiscoveryMode, NodeInfo, TopicId};
use p2panda_net::{Network, NetworkBuilder};
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use p2panda_sync::managers::topic_sync_manager::TopicSyncManagerConfig;
use rand_chacha::rand_core::SeedableRng;
use sqlx::{migrate::Migrator, sqlite};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{Notify, RwLock},
};
use tracing::{error, info, warn};

pub type TopicSyncManager = p2panda_sync::managers::topic_sync_manager::TopicSyncManager<
    TopicId,
    p2panda_store::SqliteStore<LogId, ReflectionExtensions>,
    TopicStore,
    LogId,
    ReflectionExtensions,
>;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum MessageType {
    Ephemeral(EphemerialOperation),
    AuthorEphemeral(EphemerialOperation),
}

#[derive(Debug)]
pub struct NodeInner {
    pub(crate) runtime: Runtime,
    pub(crate) operation_store: OperationStore,
    pub(crate) topic_store: TopicStore,
    pub(crate) private_key: PrivateKey,
    pub(crate) network_id: Hash,
    pub(crate) network: RwLock<Option<Network<TopicSyncManager>>>,
    pub(crate) network_notifier: Notify,
}

static RELAY_URL: LazyLock<iroh::RelayUrl> = LazyLock::new(|| {
    "https://euc1-1.relay.n0.iroh-canary.iroh.link"
        .parse()
        .expect("valid relay URL")
});

static BOOTSTRAP_NODE: LazyLock<NodeInfo> = LazyLock::new(|| {
    let endpoint_addr = iroh::EndpointAddr::new(
        "7ccdbeed587a8ec8c71cdc9b98e941ac597e11b0216aac1387ef81089a4930b2"
            .parse()
            .expect("valid bootstrap node id"),
    )
    .with_relay_url(RELAY_URL.clone());
    NodeInfo::from(endpoint_addr).bootstrap()
});

impl NodeInner {
    pub async fn new(
        network_id: Hash,
        private_key: PrivateKey,
        db_location: Option<&Path>,
        connection_mode: ConnectionMode,
    ) -> Result<Self, NodeError> {
        let runtime = Builder::new_multi_thread().enable_all().build()?;

        let _guard = runtime.enter();

        let connection_options = sqlx::sqlite::SqliteConnectOptions::new()
            .shared_cache(true)
            .create_if_missing(true);
        let connection_options = if let Some(db_location) = db_location {
            let db_file = db_location.join("database.sqlite");
            info!("Database file location: {db_file:?}");
            connection_options.filename(db_file)
        } else {
            connection_options.in_memory(true)
        };

        let pool = if db_location.is_some() {
            sqlx::sqlite::SqlitePool::connect_with(connection_options).await?
        } else {
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

        let network = match connection_mode {
            ConnectionMode::None => None,
            ConnectionMode::Bluetooth => {
                unimplemented!("Bluetooth is currently not implemented")
            }
            ConnectionMode::Network => {
                setup_network(&private_key, &network_id, &topic_store, &operation_store).await
            }
        };

        Ok(Self {
            runtime,
            operation_store,
            topic_store,
            private_key,
            network_id,
            network: RwLock::new(network),
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
                setup_network(
                    &self.private_key,
                    &self.network_id,
                    &self.topic_store,
                    &self.operation_store,
                )
                .await
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
    ) -> Result<Subscription<T>, TopicError> {
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

        Ok(Subscription::new(self, id, subscribable_topic).await)
    }

    pub async fn delete_topic(self: Arc<Self>, id: TopicId) -> Result<(), TopicError> {
        self.topic_store.delete_topic(&id).await?;
        Ok(())
    }
}

async fn setup_network(
    private_key: &PrivateKey,
    network_id: &Hash,
    topic_store: &TopicStore,
    operation_store: &OperationStore,
) -> Option<Network<TopicSyncManager>> {
    let address_book = MemoryAddressBook::new(rand_chacha::ChaCha20Rng::from_os_rng());

    if let Err(error) = address_book.insert_node_info(BOOTSTRAP_NODE.clone()).await {
        error!("Failed to add bootstrap node to the address book: {error}");
    }

    let sync_conf = TopicSyncManagerConfig {
        store: operation_store.clone_inner(),
        topic_map: topic_store.clone(),
    };
    let network = NetworkBuilder::new(network_id.into())
        .private_key(private_key.clone())
        .mdns(MdnsDiscoveryMode::Active)
        .relay(RELAY_URL.clone())
        .build(address_book, sync_conf)
        .await;

    if let Err(error) = network {
        warn!("Failed to startup network: {error}");
        None
    } else {
        network.ok()
    }
}
