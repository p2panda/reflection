use std::fmt;
use std::hash::Hash as StdHash;
use std::str::FromStr;
use std::sync::Arc;

use crate::node_inner::NodeInner;
use crate::operation_store::CreationError;
use crate::subscription_inner::SubscriptionInner;

use chrono::{DateTime, Utc};
use p2panda_core::{Hash, HashError, PublicKey};
use p2panda_net::{ToNetwork, TopicId};
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};
use sqlx::{
    Decode, Encode, FromRow, Sqlite, Type,
    encode::IsNull,
    error::BoxDynError,
    sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
};
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinError};
use tracing::error;

#[derive(Copy, Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct DocumentId(Hash);

impl DocumentId {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl TopicQuery for DocumentId {}

impl TopicId for DocumentId {
    fn id(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }
}

impl From<[u8; 32]> for DocumentId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(Hash::from_bytes(bytes))
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

impl TryFrom<&[u8]> for DocumentId {
    type Error = HashError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Ok(Hash::try_from(value)?.into())
    }
}

impl Type<Sqlite> for DocumentId {
    fn type_info() -> SqliteTypeInfo {
        <&[u8] as Type<Sqlite>>::type_info()
    }

    fn compatible(ty: &SqliteTypeInfo) -> bool {
        <&[u8] as Type<Sqlite>>::compatible(ty)
    }
}

impl<'q> Encode<'q, Sqlite> for &'q DocumentId {
    fn encode_by_ref(
        &self,
        args: &mut Vec<SqliteArgumentValue<'q>>,
    ) -> Result<IsNull, BoxDynError> {
        <&[u8] as Encode<Sqlite>>::encode_by_ref(&self.as_bytes(), args)
    }
}

impl Decode<'_, Sqlite> for DocumentId {
    fn decode(value: SqliteValueRef<'_>) -> Result<Self, BoxDynError> {
        Ok(DocumentId::try_from(<&[u8] as Decode<Sqlite>>::decode(
            value,
        )?)?)
    }
}

#[derive(Debug, FromRow)]
pub struct Document {
    #[sqlx(rename = "document_id")]
    pub id: DocumentId,
    #[sqlx(default)]
    pub name: Option<String>,
    pub last_accessed: Option<DateTime<Utc>>,
    #[sqlx(skip)]
    pub authors: Vec<Author>,
}

#[derive(Debug)]
pub struct Author {
    pub public_key: PublicKey,
    pub last_seen: Option<DateTime<Utc>>,
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error(transparent)]
    DocumentStore(#[from] sqlx::Error),
    #[error(transparent)]
    OperationStore(#[from] CreationError),
    #[error(transparent)]
    Encode(#[from] p2panda_core::cbor::EncodeError),
    #[error(transparent)]
    Send(#[from] mpsc::error::SendError<ToNetwork>),
    #[error(transparent)]
    Runtime(#[from] JoinError),
}

pub trait SubscribableDocument: Sync + Send {
    fn bytes_received(&self, author: PublicKey, data: Vec<u8>);
    fn author_joined(&self, author: PublicKey);
    fn author_left(&self, author: PublicKey);
    fn ephemeral_bytes_received(&self, author: PublicKey, data: Vec<u8>);
}

pub struct Subscription {
    pub(crate) inner: Arc<SubscriptionInner>,
}

impl Subscription {
    pub(crate) async fn new(
        node: Arc<NodeInner>,
        id: DocumentId,
        document: Arc<impl SubscribableDocument + 'static>,
    ) -> Self {
        let inner = SubscriptionInner::new(node, id, document).await;
        Self { inner }
    }

    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.send_delta(data).await })
            .await?
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.send_snapshot(data).await })
            .await?
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.send_ephemeral(data).await })
            .await?
    }

    pub async fn unsubscribe(self) -> Result<(), DocumentError> {
        let Subscription { inner } = self;

        inner
            .node
            .clone()
            .runtime
            .spawn(async move { inner.unsubscribe().await })
            .await?
    }

    /// Set the name for a given document
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), DocumentError> {
        let inner = self.inner.clone();
        self.inner
            .node
            .runtime
            .spawn(async move { inner.set_name(name).await })
            .await?
    }
}
