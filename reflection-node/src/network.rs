use std::sync::LazyLock;

use p2panda_net::Discovery;
use p2panda_net::discovery::DiscoveryError;
use thiserror::Error;
use tracing::error;

use p2panda_core::Hash;
use p2panda_core::PrivateKey;
use p2panda_net::address_book::{AddressBook, AddressBookError};
use p2panda_net::addrs::NodeInfo;
use p2panda_net::gossip::{Gossip, GossipError};
use p2panda_net::iroh_endpoint::{Endpoint, EndpointAddr, EndpointError, RelayUrl};
use p2panda_net::iroh_mdns::{MdnsDiscovery, MdnsDiscoveryError, MdnsDiscoveryMode};

use crate::operation::ReflectionExtensions;
use crate::operation_store::OperationStore;
use crate::topic_store::{LogId, TopicStore};

static RELAY_URL: LazyLock<RelayUrl> = LazyLock::new(|| {
    "https://euc1-1.relay.n0.iroh-canary.iroh.link"
        .parse()
        .expect("valid relay URL")
});

static BOOTSTRAP_NODE: LazyLock<NodeInfo> = LazyLock::new(|| {
    let endpoint_addr = EndpointAddr::new(
        "9f63a15ab95959a992af96bf72fbc3e7dc98eeb4799f788bb07b20125053e795"
            .parse()
            .expect("valid bootstrap node id"),
    )
    .with_relay_url(RELAY_URL.clone());
    NodeInfo::from(endpoint_addr).bootstrap()
});

pub type LogSync = p2panda_net::sync::LogSync<
    p2panda_store::SqliteStore<LogId, ReflectionExtensions>,
    LogId,
    ReflectionExtensions,
    TopicStore,
>;
pub type LogSyncError = p2panda_net::sync::LogSyncError<ReflectionExtensions>;

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
    Discovery(#[from] DiscoveryError),
    #[error(transparent)]
    Endpoint(#[from] EndpointError),
}

#[allow(dead_code)]
pub struct Network {
    pub(crate) mdns_discovery: MdnsDiscovery,
    pub(crate) discovery: Discovery,
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

        let discovery = Discovery::builder(address_book.clone(), endpoint.clone())
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
            discovery,
            gossip,
            log_sync,
            endpoint,
        })
    }
}
