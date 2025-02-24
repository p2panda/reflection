use std::fmt;
use std::hash::Hash as StdHash;

use p2panda_core::Hash;
use p2panda_net::TopicId;
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct Document(Hash);

impl TopicQuery for Document {}

impl TopicId for Document {
    fn id(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }
}

impl From<Hash> for Document {
    fn from(document_id: Hash) -> Self {
        Self(document_id)
    }
}

impl From<&Document> for Hash {
    fn from(value: &Document) -> Self {
        value.0
    }
}

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
