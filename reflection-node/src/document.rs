use std::fmt;
use std::hash::Hash as StdHash;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use p2panda_core::PublicKey;
use p2panda_net::TopicId;
use p2panda_spaces::types::ACTOR_ID_SIZE;
use p2panda_spaces::{ActorId, types::ActorIdError};
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};
use sqlx::{
    Decode, Encode, FromRow, Sqlite, Type,
    encode::IsNull,
    error::BoxDynError,
    sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
};

pub type DocumentIdError = ActorIdError;

#[derive(Copy, Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct DocumentId(ActorId);

impl DocumentId {
    pub fn as_bytes(&self) -> &[u8; ACTOR_ID_SIZE] {
        self.0.as_bytes()
    }
}

impl TopicQuery for DocumentId {}

impl TopicId for DocumentId {
    fn id(&self) -> [u8; 32] {
        *self.0.as_bytes()
    }
}

impl From<ActorId> for DocumentId {
    fn from(actor_id: ActorId) -> Self {
        Self(actor_id)
    }
}

impl From<DocumentId> for ActorId {
    fn from(document_id: DocumentId) -> Self {
        document_id.0
    }
}

impl TryFrom<[u8; ACTOR_ID_SIZE]> for DocumentId {
    type Error = ActorIdError;

    fn try_from(bytes: [u8; ACTOR_ID_SIZE]) -> Result<Self, Self::Error> {
        Ok(Self(ActorId::from_bytes(&bytes)?))
    }
}

impl TryFrom<&[u8]> for DocumentId {
    type Error = ActorIdError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self(ActorId::try_from(bytes)?))
    }
}

impl FromStr for DocumentId {
    type Err = ActorIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(ActorId::from_str(value)?))
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
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
        <&[u8] as Encode<Sqlite>>::encode_by_ref(&self.as_bytes().as_slice(), args)
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

pub trait SubscribableDocument: Sync + Send {
    fn bytes_received(&self, author: PublicKey, data: Vec<u8>);
    fn authors_joined(&self, authors: Vec<PublicKey>);
    fn author_set_online(&self, author: PublicKey, is_online: bool);
    fn ephemeral_bytes_received(&self, author: PublicKey, data: Vec<u8>);
}
