use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::PublicKey;
use p2panda_store::MemoryStore;
use p2panda_sync::log_sync::TopicLogMap;
use tokio::sync::RwLock;

use crate::document::Document;
use crate::operation::AardvarkExtensions;

pub type LogId = Document;

#[derive(Clone, Debug)]
pub struct DocumentStore {
    inner: Arc<RwLock<DocumentStoreInner>>,
}

#[derive(Debug)]
struct DocumentStoreInner {
    authors: HashMap<PublicKey, Vec<Document>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DocumentStoreInner {
                authors: HashMap::new(),
            })),
        }
    }

    pub async fn add_author(&self, document: Document, public_key: PublicKey) -> Result<()> {
        let mut store = self.inner.write().await;
        store
            .authors
            .entry(public_key)
            .and_modify(|documents| {
                if !documents.contains(&document) {
                    documents.push(document);
                }
            })
            .or_insert(vec![document]);
        Ok(())
    }
}

#[async_trait]
impl TopicLogMap<Document, LogId> for DocumentStore {
    async fn get(&self, topic: &Document) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let store = &self.inner.read().await;
        let mut result = HashMap::<PublicKey, Vec<Document>>::new();

        for (public_key, documents) in &store.authors {
            if documents.contains(topic) {
                result
                    .entry(*public_key)
                    .and_modify(|logs| logs.push(*topic))
                    .or_insert(vec![*topic]);
            }
        }

        Some(result)
    }
}

pub type OperationStore = MemoryStore<LogId, AardvarkExtensions>;
