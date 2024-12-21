use std::collections::HashMap;
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use async_trait::async_trait;
use p2panda_core::{cbor, Hash, PublicKey};
use p2panda_net::TopicId;
use p2panda_sync::{log_sync::TopicLogMap, TopicQuery};
use serde::{Deserialize, Serialize};

use crate::operation::LogId;

#[derive(Clone, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
pub struct TextDocument(pub PublicKey, pub u64);

impl TextDocument {
    pub fn hash(&self) -> Hash {
        let bytes = cbor::encode_cbor(self).expect("can encode as cbor bytes");
        Hash::new(bytes)
    }
}

impl TopicQuery for TextDocument {}

impl TopicId for TextDocument {
    fn id(&self) -> [u8; 32] {
        self.hash().as_bytes().to_owned()
    }
}

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

    pub fn write(&self) -> RwLockWriteGuard<TextDocumentStoreInner> {
        self.inner.write().expect("acquire write lock")
    }
}

#[derive(Clone, Debug)]
pub struct TextDocumentStoreInner {
    pub authors: HashMap<PublicKey, Vec<TextDocument>>,
}

#[async_trait]
impl TopicLogMap<TextDocument, LogId> for TextDocumentStore {
    async fn get(&self, topic: &TextDocument) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let authors = &self.inner.read().unwrap().authors;
        let mut result = HashMap::<PublicKey, Vec<LogId>>::new();

        for (public_key, documents) in authors {
            if documents.contains(&topic) {
                result
                    .entry(*public_key)
                    .and_modify(|logs| logs.push(topic.hash()))
                    .or_insert(vec![topic.hash()]);
            }
        }

        Some(result)
    }
}
