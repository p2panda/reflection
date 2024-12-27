use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use async_trait::async_trait;
use p2panda_core::PublicKey;
use p2panda_sync::log_sync::TopicLogMap;

use crate::operation::LogId;
use crate::topics::{AardvarkTopics, TextDocument};

pub type ShortCode = [char; 6];

#[derive(Clone, Debug)]
pub struct TextDocumentStore {
    inner: Arc<RwLock<TextDocumentStoreInner>>,
}

impl Default for TextDocumentStore {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(TextDocumentStoreInner {
                authors: HashMap::new(),
            })),
        }
    }
}

impl TextDocumentStore {
    pub fn write(&self) -> RwLockWriteGuard<TextDocumentStoreInner> {
        self.inner.write().expect("acquire write lock")
    }
}

#[derive(Clone, Debug)]
pub struct TextDocumentStoreInner {
    pub authors: HashMap<PublicKey, Vec<TextDocument>>,
}

#[async_trait]
impl TopicLogMap<AardvarkTopics, LogId> for TextDocumentStore {
    async fn get(&self, topic: &AardvarkTopics) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let text_document = match topic {
            // When discovering documents we don't want any sync sessions to occur, this is a
            // little hack to make sure that is the case, as if both peers resolve a topic to
            // "None" then the sync session will naturally end.
            AardvarkTopics::DiscoveryCode(_) => return None,
            AardvarkTopics::TextDocument(text_document) => text_document,
        };

        let authors = &self.inner.read().unwrap().authors;
        let mut result = HashMap::<PublicKey, Vec<LogId>>::new();

        for (public_key, documents) in authors {
            if documents.contains(text_document) {
                result
                    .entry(*public_key)
                    .and_modify(|logs| logs.push(text_document.hash()))
                    .or_insert(vec![text_document.hash()]);
            }
        }

        Some(result)
    }
}
