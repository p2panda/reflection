use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use p2panda_core::PublicKey;
use p2panda_sync::log_sync::TopicLogMap;
use tokio::sync::RwLock;

use crate::topic::TextDocument;

#[derive(Clone, Debug)]
pub struct TextDocumentStore {
    inner: Arc<RwLock<TextDocumentStoreInner>>,
}

impl TextDocumentStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TextDocumentStoreInner {
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

#[derive(Clone, Debug)]
struct TextDocumentStoreInner {
    authors: HashMap<PublicKey, Vec<TextDocument>>,
}

pub type LogId = TextDocument;

#[async_trait]
impl TopicLogMap<TextDocument, LogId> for TextDocumentStore {
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
