use std::fmt;
use std::hash::Hash as StdHash;
use std::str::FromStr;

use p2panda_core::{Hash, HashError, PublicKey};
use p2panda_net::TopicId;
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct DocumentId(Hash);

impl TopicQuery for DocumentId {}

impl TopicId for DocumentId {
    fn id(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }
}

impl From<Hash> for DocumentId {
    fn from(document_id: Hash) -> Self {
        Self(document_id)
    }
}

impl From<DocumentId> for Hash {
    fn from(document: DocumentId) -> Self {
        document.0
    }
}

impl From<&DocumentId> for Hash {
    fn from(value: &DocumentId) -> Self {
        value.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for DocumentId {
    type Err = HashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Hash::from_str(value)?.into())
    }
}

pub trait SubscribableDocument: Sync + Send {
    fn bytes_received(&self, author: PublicKey, data: &[u8]);
}
