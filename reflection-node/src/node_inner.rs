use std::ops::DerefMut;
use std::path::Path;
use std::sync::Arc;

use crate::document::{DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::document_store::DocumentStore;
use crate::ephemerial_operation::EphemerialOperation;
use crate::node::{ConnectionMode, NodeError};
use crate::operation_store::OperationStore;
use crate::persistent_operation::PersistentOperation;
use crate::utils::CombinedMigrationSource;

use p2panda_core::{Hash, PrivateKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{Network, NetworkBuilder, SyncConfiguration};
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use p2panda_sync::log_sync::LogSyncProtocol;
use sqlx::{migrate::Migrator, sqlite};
use tokio::{
    runtime::{Builder, Runtime},
    sync::{Notify, RwLock},
};
use tracing::{info, warn};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum MessageType {
    Persistent(PersistentOperation),
    Ephemeral(EphemerialOperation),
    AuthorEphemeral(EphemerialOperation),
}

#[derive(Debug)]
pub struct NodeInner {
    pub(crate) runtime: Runtime,
    pub(crate) operation_store: OperationStore,
    pub(crate) document_store: DocumentStore,
    pub(crate) private_key: PrivateKey,
    pub(crate) network_id: Hash,
    pub(crate) network: RwLock<Option<Network<DocumentId>>>,
    pub(crate) network_notifier: Notify,
}

//const RELAY_URL: &str = "https://staging-euw1-1.relay.iroh.network/";
//const BOOTSTRAP_NODE_ID: &str = "d825a2f929f935efcd6889bed5c3f5510b40f014969a729033d3fb7e33b97dbe";

impl NodeInner {
    pub async fn new(
        network_id: Hash,
        private_key: PrivateKey,
        db_location: Option<&Path>,
        connection_mode: ConnectionMode,
    ) -> Result<Self, NodeError> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

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

        // Run migration for p2panda OperationStore and for the our DocumentStore
        Migrator::new(CombinedMigrationSource::new(vec![
            operation_store_migrations(),
            sqlx::migrate!(),
        ]))
        .await?
        .run(&pool)
        .await?;

        let operation_store = OperationStore::new(pool.clone());
        let document_store = DocumentStore::new(pool);

        let network = match connection_mode {
            ConnectionMode::None => None,
            ConnectionMode::Bluetooth => {
                unimplemented!("Bluetooth is currently not implemented")
            }
            ConnectionMode::Network => {
                setup_network(&private_key, &network_id, &document_store, &operation_store).await
            }
        };

        Ok(Self {
            runtime,
            operation_store,
            document_store,
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
                    &self.document_store,
                    &self.operation_store,
                )
                .await
            }
        };

        let old_network = std::mem::replace(network_guard.deref_mut(), network);

        if let Some(old_network) = old_network {
            // FIXME: For some reason we shutdown before the bye message is actually send
            // This doesn't happen when shutting down the entire node, maybe because tokio is more busy here?
            if let Err(error) = old_network.shutdown().await {
                warn!("Failed to shutdown network: {error}");
            }
        }
    }

    pub async fn shutdown(&self) {
        // Wake up all subscriptions that may still exist
        self.network_notifier.notify_waiters();
        if let Some(network) = self.network.write().await.take()
            && let Err(error) = network.shutdown().await {
                warn!("Failed to shutdown network: {error}");
            }
    }

    pub async fn subscribe<T: SubscribableDocument + 'static>(
        self: Arc<Self>,
        document_id: DocumentId,
        document: Arc<T>,
    ) -> Result<Subscription<T>, DocumentError> {
        self.document_store.add_document(&document_id).await?;
        // Add ourselves as an author to the document store.
        self.document_store
            .add_author(&document_id, &self.private_key.public_key())
            .await?;
        let stored_operations = self
            .document_store
            .operations_for_document(&self.operation_store, &document_id)
            .await?;

        for operation in stored_operations {
            // Send all stored operation bytes to the app,
            // it doesn't matter if the app already knows some or all of them
            if let Some(body) = operation.body {
                document.bytes_received(operation.header.public_key, body.to_bytes());
            }
        }

        Ok(Subscription::new(self, document_id, document).await)
    }
}

async fn setup_network(
    private_key: &PrivateKey,
    network_id: &Hash,
    document_store: &DocumentStore,
    operation_store: &OperationStore,
) -> Option<Network<DocumentId>> {
    let sync_config = {
        let sync = LogSyncProtocol::new(document_store.clone(), operation_store.clone_inner());
        SyncConfiguration::<DocumentId>::new(sync)
    };

    let network = NetworkBuilder::new(network_id.into())
        .private_key(private_key.clone())
        .discovery(LocalDiscovery::new())
        // NOTE(glyph): Internet networking is disabled until we can fix the
        // more-than-two-peers gossip issue.
        //
        //.relay(RELAY_URL.parse().expect("valid relay URL"), false, 0)
        //.direct_address(
        //    BOOTSTRAP_NODE_ID.parse().expect("valid node ID"),
        //    vec![],
        //    None,
        //)
        .gossip(GossipConfig {
            // FIXME: This is a temporary workaround to account for larger delta patches (for
            // example when the user Copy & Pastes a big chunk of text).
            //
            // Related issue: https://github.com/p2panda/reflection/issues/24
            max_message_size: 512_000,
        })
        .sync(sync_config)
        .build()
        .await;

    if let Err(error) = network {
        warn!("Failed to startup network: {error}");
        None
    } else {
        network.ok()
    }
}
