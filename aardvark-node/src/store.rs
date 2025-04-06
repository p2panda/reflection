use std::collections::{HashMap, HashSet};
use std::hash::Hash as StdHash;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::PublicKey;
use p2panda_store::MemoryStore;
use p2panda_sync::log_sync::TopicLogMap;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::document::DocumentId;
use crate::operation::{AardvarkExtensions, LogType};

#[derive(Clone, Debug)]
pub struct DocumentStore {
    inner: Arc<RwLock<DocumentStoreInner>>,
}

#[derive(Debug)]
struct DocumentStoreInner {
    authors: HashMap<PublicKey, HashSet<DocumentId>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DocumentStoreInner {
                authors: HashMap::new(),
            })),
        }
    }

    pub async fn add_author(&self, document: DocumentId, public_key: PublicKey) -> Result<()> {
        let mut store = self.inner.write().await;
        store
            .authors
            .entry(public_key)
            .and_modify(|documents| {
                documents.insert(document);
            })
            .or_insert(HashSet::from([document]));
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct LogId(LogType, DocumentId);

impl LogId {
    pub fn new(log_type: LogType, document: &DocumentId) -> Self {
        Self(log_type, *document)
    }
}

#[async_trait]
impl TopicLogMap<DocumentId, LogId> for DocumentStore {
    async fn get(&self, topic: &DocumentId) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let store = &self.inner.read().await;
        let mut result = HashMap::<PublicKey, Vec<LogId>>::new();

        for (public_key, documents) in &store.authors {
            if documents.contains(topic) {
                // We maintain two logs per author per document.
                let log_ids = [
                    LogId::new(LogType::Delta, topic),
                    LogId::new(LogType::Snapshot, topic),
                ];

                result
                    .entry(*public_key)
                    .and_modify(|logs| {
                        logs.extend_from_slice(&log_ids);
                    })
                    .or_insert(log_ids.into());
            }
        }

        Some(result)
    }
}

pub type OperationStore = MemoryStore<LogId, AardvarkExtensions>;
