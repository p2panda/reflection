use std::collections::{HashMap, HashSet};
use std::hash::Hash as StdHash;
use std::sync::Arc;

use async_trait::async_trait;
use p2panda_core::PublicKey;
use p2panda_store::SqliteStore;
use p2panda_sync::log_sync::TopicLogMap;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::document::DocumentId;
use crate::operation::{AardvarkExtensions, LogType};

#[derive(Clone, Debug)]
pub struct DocumentStore {
    documents: Arc<RwLock<HashMap<DocumentId, HashSet<PublicKey>>>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_author(&self, document: DocumentId, public_key: PublicKey) {
        let mut documents = self.documents.write().await;
        documents
            .entry(document)
            .and_modify(|documents| {
                documents.insert(public_key);
            })
            .or_insert(HashSet::from([public_key]));
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
        let documents = self.documents.read().await;
        let authors = documents.get(topic)?.clone();
        let log_ids = [
            LogId::new(LogType::Delta, topic),
            LogId::new(LogType::Snapshot, topic),
        ];
        Some(
            authors
                .into_iter()
                .map(|author| (author, log_ids.to_vec()))
                .collect(),
        )
    }
}

pub type OperationStore = SqliteStore<LogId, AardvarkExtensions>;
