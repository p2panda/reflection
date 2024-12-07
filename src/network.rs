use anyhow::Result;
use p2panda_core::{Extension, Hash, PrivateKey, PruneFlag, PublicKey};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::TopicId;
use p2panda_net::{NetworkBuilder, SyncConfiguration};
use p2panda_store::MemoryStore;
use p2panda_sync::log_sync::LogSyncProtocol;
use p2panda_sync::{TopicMap, TopicQuery};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
struct TextDocument(Hash);

impl TopicQuery for TextDocument {}

impl TopicId for TextDocument {
    fn id(&self) -> [u8; 32] {
        self.0.into()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AarvdarkExtensions {
    prune_flag: PruneFlag,
}

impl Extension<PruneFlag> for AarvdarkExtensions {
    fn extract(&self) -> Option<PruneFlag> {
        Some(self.prune_flag.clone())
    }
}

type LogId = TextDocument;

#[derive(Debug)]
struct Topic2TextDocument {}

#[async_trait]
impl TopicMap<TextDocument, LogId> for Topic2TextDocument {
    async fn get(&self, topic: &TextDocument) -> Option<LogId> {
        Some(topic.clone())
    }
}

pub async fn run() -> Result<()> {
    let network_id = Hash::new(b"aardvark <3");
    let private_key = PrivateKey::new();

    let store = MemoryStore::<LogId, AarvdarkExtensions>::new();

    let topic_map = Topic2TextDocument {};
    let sync = LogSyncProtocol::new(topic_map, store);
    let sync_config = SyncConfiguration::new(sync);

    let mut network = NetworkBuilder::new(*network_id.as_bytes())
        .sync(sync_config)
        .private_key(private_key)
        .discovery(LocalDiscovery::new()?)
        .build()
        .await?;

    let test_document = TextDocument(Hash::new(b"my first doc <3"));
    let (tx, rx, ready) = network.subscribe(test_document).await?;

    Ok(())
}
