use std::fmt;
use std::hash::Hash as StdHash;
use std::str::FromStr;
use std::sync::Arc;

use crate::author_tracker::AuthorMessage;
use crate::ephemerial_operation::EphemerialOperation;
use crate::node_inner::MessageType;
use crate::node_inner::NodeInner;
use crate::operation::LogType;
use crate::operation_store::CreationError;
use crate::persistent_operation::PersistentOperation;
use chrono::{DateTime, Utc};
use p2panda_core::cbor::encode_cbor;
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
use tokio::{
    sync::mpsc,
    task::{AbortHandle, JoinError},
};
use tracing::{error, info};

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
    pub(crate) tx: mpsc::Sender<ToNetwork>,
    pub(crate) id: DocumentId,
    pub(crate) node: Arc<NodeInner>,
    pub(crate) abort_handles: Vec<AbortHandle>,
}

impl Subscription {
    pub async fn send_delta(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let node = self.node.clone();
        let document_id = self.id;
        let operation = self
            .node
            .runtime
            .spawn(async move {
                // Append one operation to our "ephemeral" delta log.
                node.operation_store
                    .create_operation(
                        &node.private_key,
                        LogType::Delta,
                        Some(document_id),
                        Some(&data),
                        false,
                    )
                    .await
            })
            .await??;

        info!("Delta operation sent for document with id {}", self.id);

        let bytes = encode_cbor(&MessageType::Persistent(PersistentOperation::new(
            operation,
        )))?;

        // Broadcast operation on gossip overlay.
        self.tx.send(ToNetwork::Message { bytes }).await?;

        Ok(())
    }

    pub async fn send_snapshot(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let node = self.node.clone();
        let document_id = self.id;

        let operation = self
            .node
            .runtime
            .spawn(async move {
                // Append an operation to our "snapshot" log and set the prune flag to
                // true. This will remove previous snapshots.
                //
                // Snapshots are not broadcasted on the gossip overlay as they would be
                // too large. Peers will sync them up when they join the document.
                node.operation_store
                    .create_operation(
                        &node.private_key,
                        LogType::Snapshot,
                        Some(document_id),
                        Some(&data),
                        true,
                    )
                    .await?;

                // Append an operation to our "ephemeral" delta log and set the prune
                // flag to true.
                //
                // This signals removing all previous "delta" operations now. This is
                // some sort of garbage collection whenever we snapshot. Snapshots
                // already contain all history, there is no need to keep duplicate
                // "delta" data around.
                node.operation_store
                    .create_operation(
                        &node.private_key,
                        LogType::Delta,
                        Some(document_id),
                        None,
                        true,
                    )
                    .await
            })
            .await??;

        info!("Snapshot saved for document with id {}", self.id);

        let bytes = encode_cbor(&MessageType::Persistent(PersistentOperation::new(
            operation,
        )))?;

        // Broadcast operation on gossip overlay.
        self.tx.send(ToNetwork::Message { bytes }).await?;

        Ok(())
    }

    pub async fn send_ephemeral(&self, data: Vec<u8>) -> Result<(), DocumentError> {
        let operation = EphemerialOperation::new(data, &self.node.private_key);

        let bytes = encode_cbor(&MessageType::Ephemeral(operation))?;
        self.tx.send(ToNetwork::Message { bytes }).await?;

        Ok(())
    }

    pub async fn unsubscribe(self) -> Result<(), DocumentError> {
        let node = self.node.clone();
        let document_id = self.id;
        self.node
            .runtime
            .spawn(async move {
                node.document_store
                    .set_last_accessed_for_document(&document_id, Some(Utc::now()))
                    .await
            })
            .await??;

        // Abort all tokio tasks created during subscription
        for handle in self.abort_handles {
            handle.abort();
        }

        // Send good bye message to the network
        if let Err(error) = AuthorMessage::Bye
            .send(&self.tx, &self.node.private_key)
            .await
        {
            error!("Failed to sent bye message to the network: {error}");
        }

        info!("Unsubscribed from document {document_id}");

        Ok(())
    }

    /// Set the name for a given document
    ///
    /// This information will be written to the database
    pub async fn set_name(&self, name: Option<String>) -> Result<(), DocumentError> {
        let node = self.node.clone();
        let document_id = self.id;
        self.node
            .runtime
            .spawn(async move {
                node.document_store
                    .set_name_for_document(&document_id, name)
                    .await
            })
            .await??;

        Ok(())
    }
}
