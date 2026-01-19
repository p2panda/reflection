use std::sync::LazyLock;

use thiserror::Error;
use tracing::error;

use p2panda_core::Hash;
use p2panda_core::PrivateKey;
use p2panda_net::address_book::{AddressBook, AddressBookError};
use p2panda_net::gossip::{Gossip, GossipError};
use p2panda_net::iroh_endpoint::{Endpoint, EndpointError};
use p2panda_net::iroh_mdns::{MdnsDiscovery, MdnsDiscoveryError, MdnsDiscoveryMode};
use p2panda_net::{TopicId, addrs::NodeInfo};

use crate::operation::ReflectionExtensions;
use crate::operation_store::OperationStore;
use crate::topic_store::{LogId, TopicStore};

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

type TopicSyncManager = p2panda_sync::manager::TopicSyncManager<
    TopicId,
    p2panda_store::SqliteStore<LogId, ReflectionExtensions>,
    TopicStore,
    LogId,
    ReflectionExtensions,
>;
pub type LogSync = p2panda_net::sync::LogSync<
    p2panda_store::SqliteStore<LogId, ReflectionExtensions>,
    LogId,
    ReflectionExtensions,
    TopicStore,
>;
pub type LogSyncError = p2panda_net::sync::LogSyncError<TopicSyncManager>;
pub type SyncHandle = p2panda_net::sync::SyncHandle<TopicSyncManager>;
pub type SyncHandleError = p2panda_net::sync::SyncHandleError<TopicSyncManager>;

#[derive(Error, Debug)]
pub enum NetworkError {
    #[error(transparent)]
    Gossip(#[from] GossipError),
    #[error(transparent)]
    LogSync(#[from] LogSyncError),
    #[error(transparent)]
    AddressBook(#[from] AddressBookError),
    #[error(transparent)]
    MdnsDiscovery(#[from] MdnsDiscoveryError),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
}

#[allow(dead_code)]
pub struct Network {
    pub(crate) mdns_discovery: MdnsDiscovery,
    pub(crate) gossip: Gossip,
    pub(crate) log_sync: LogSync,
    pub(crate) endpoint: Endpoint,
}

// FIXME: Endpoint, LogSync, MdnsDiscovery, and Gossip should implement debug
impl std::fmt::Debug for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Network").finish()
    }
}

impl Network {
    pub async fn new(
        private_key: &PrivateKey,
        network_id: &Hash,
        topic_store: &TopicStore,
        operation_store: &OperationStore,
    ) -> Result<Self, NetworkError> {
        let address_book = AddressBook::builder().spawn().await?;

        if let Err(error) = address_book.insert_node_info(BOOTSTRAP_NODE.clone()).await {
            error!("Failed to add bootstrap node to the address book: {error}");
        }

        let endpoint = Endpoint::builder(address_book.clone())
            .network_id(network_id.into())
            .private_key(private_key.clone())
            .relay_url(RELAY_URL.clone())
            .spawn()
            .await?;

        let mdns_discovery = MdnsDiscovery::builder(address_book.clone(), endpoint.clone())
            .mode(MdnsDiscoveryMode::Active)
            .spawn()
            .await?;
        let gossip = Gossip::builder(address_book.clone(), endpoint.clone())
            .spawn()
            .await?;
        let log_sync = LogSync::builder(
            operation_store.clone_inner(),
            topic_store.clone(),
            endpoint.clone(),
            gossip.clone(),
        )
        .spawn()
        .await?;

        Ok(Network {
            mdns_discovery,
            gossip,
            log_sync,
            endpoint,
        })
    }
}
