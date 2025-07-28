use std::{convert::Infallible, fmt};
use std::hash::Hash as StdHash;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use p2panda_core::{PublicKey, identity::PUBLIC_KEY_LEN};
use p2panda_net::TopicId;
use p2panda_spaces::types::ActorId;
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};
use sqlx::{
    Decode, Encode, FromRow, Sqlite, Type,
    encode::IsNull,
    error::BoxDynError,
    sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct DocumentId(PublicKey);

impl DocumentId {
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl TopicQuery for DocumentId {}

impl TopicId for DocumentId {
    fn id(&self) -> [u8; PUBLIC_KEY_LEN] {
        *self.0.as_bytes()
    }
}

impl From<[u8; PUBLIC_KEY_LEN]> for DocumentId {
    fn from(bytes: [u8; PUBLIC_KEY_LEN]) -> Self {
        // @TODO: implement TryFrom and handle errors.
        Self(PublicKey::from_bytes(&bytes).unwrap())
    }
}

impl From<PublicKey> for DocumentId {
    fn from(public_key: PublicKey) -> Self {
        Self(public_key)
    }
}

impl From<ActorId> for DocumentId {
    fn from(actor_id: ActorId) -> Self {
        let public_key: PublicKey = actor_id.into();
        Self(public_key)
    }
}


impl From<DocumentId> for ActorId {
    fn from(document_id: DocumentId) -> Self {
        let public_key: PublicKey = document_id.into();
        public_key.into()
    }
}

impl From<DocumentId> for PublicKey {
    fn from(document: DocumentId) -> Self {
        document.0
    }
}

impl From<&DocumentId> for PublicKey {
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
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // @TODO: handle errors.
        Ok(PublicKey::from_str(value).unwrap().into())
    }
}

impl TryFrom<&[u8]> for DocumentId {
    type Error = Infallible;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        // @TODO: handle errors.
        Ok(PublicKey::try_from(value).unwrap().into())
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

pub trait SubscribableDocument: Sync + Send {
    fn bytes_received(&self, author: PublicKey, data: Vec<u8>);
    fn authors_joined(&self, authors: Vec<PublicKey>);
    fn author_set_online(&self, author: PublicKey, is_online: bool);
    fn ephemeral_bytes_received(&self, author: PublicKey, data: Vec<u8>);
}
