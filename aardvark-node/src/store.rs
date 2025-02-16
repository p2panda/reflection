use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::PublicKey;
use p2panda_store::MemoryStore;
use p2panda_sync::log_sync::TopicLogMap;
use tokio::sync::RwLock;

use crate::operation::AardvarkExtensions;
use crate::topic::TextDocument;

pub type LogId = TextDocument;

#[derive(Clone, Debug)]
pub struct DocumentStore {
    inner: Arc<RwLock<DocumentStoreInner>>,
}

#[derive(Debug)]
struct DocumentStoreInner {
    authors: HashMap<PublicKey, Vec<TextDocument>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(DocumentStoreInner {
                authors: HashMap::new(),
            })),
        }
    }

    pub async fn add_author(&self, document_id: TextDocument, public_key: PublicKey) -> Result<()> {
        let mut store = self.inner.write().await;
        store
            .authors
            .entry(public_key)
            .and_modify(|documents| {
                if !documents.contains(&document_id) {
                    documents.push(document_id.clone());
                }
            })
            .or_insert(vec![document_id]);
        Ok(())
    }
}

#[async_trait]
impl TopicLogMap<TextDocument, LogId> for DocumentStore {
    async fn get(&self, topic: &TextDocument) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let store = &self.inner.read().await;
        let mut result = HashMap::<PublicKey, Vec<TextDocument>>::new();

        for (public_key, text_documents) in &store.authors {
            if text_documents.contains(topic) {
                result
                    .entry(*public_key)
                    .and_modify(|logs| logs.push(topic.clone()))
                    .or_insert(vec![topic.clone()]);
            }
        }

        Some(result)
    }
}

pub type OperationStore = MemoryStore<LogId, AardvarkExtensions>;
