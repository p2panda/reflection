use std::fmt;
use std::hash::Hash as StdHash;

use p2panda_core::Hash;
use p2panda_net::TopicId;
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct TextDocument(Hash);

impl TopicQuery for TextDocument {}

impl TopicId for TextDocument {
    fn id(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }
}

impl From<Hash> for TextDocument {
    fn from(document_id: Hash) -> Self {
        Self(document_id)
    }
}

impl From<&TextDocument> for Hash {
    fn from(value: &TextDocument) -> Self {
        value.0
    }
}

impl fmt::Display for TextDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
