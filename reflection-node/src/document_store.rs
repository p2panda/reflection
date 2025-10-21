use std::collections::HashMap;
use std::hash::Hash as StdHash;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use p2panda_core::PublicKey;
use p2panda_store::LogStore;
use p2panda_sync::log_sync::TopicLogMap;
use serde::{Deserialize, Serialize};
use sqlx::{
    Decode, Encode, FromRow, Row, Sqlite, Type,
    encode::IsNull,
    error::BoxDynError,
    sqlite::{SqliteArgumentValue, SqliteTypeInfo, SqliteValueRef},
};
use tracing::error;

use crate::document::DocumentId;
use crate::operation::{LogType, ReflectionExtensions};
use crate::operation_store::OperationStore;

#[derive(Debug, FromRow)]
pub struct StoreDocument {
    #[sqlx(rename = "document_id")]
    pub id: DocumentId,
    #[sqlx(default)]
    pub name: Option<String>,
    pub last_accessed: Option<DateTime<Utc>>,
    #[sqlx(skip)]
    pub authors: Vec<Author>,
}

#[derive(Debug, Clone)]
pub struct Author {
    pub public_key: PublicKey,
    pub last_seen: Option<DateTime<Utc>>,
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
        <&[u8] as Encode<Sqlite>>::encode_by_ref(&self.as_slice(), args)
    }
}

impl Decode<'_, Sqlite> for DocumentId {
    fn decode(value: SqliteValueRef<'_>) -> Result<Self, BoxDynError> {
        let value = <&[u8] as Decode<Sqlite>>::decode(value)?;

        Ok(DocumentId::from(TryInto::<[u8; 32]>::try_into(value)?))
    }
}

#[derive(Clone, Debug)]
pub struct DocumentStore {
    pool: sqlx::SqlitePool,
}

impl DocumentStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    async fn authors(&self, document_id: &DocumentId) -> sqlx::Result<Vec<PublicKey>> {
        let list = sqlx::query("SELECT public_key FROM authors WHERE document_id = ?")
            .bind(document_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(list
            .iter()
            .filter_map(|row| PublicKey::try_from(row.get::<&[u8], _>("public_key")).ok())
            .collect())
    }

    pub async fn documents(&self) -> sqlx::Result<Vec<StoreDocument>> {
        let mut documents: Vec<StoreDocument> =
            sqlx::query_as("SELECT document_id, name, last_accessed FROM documents")
                .fetch_all(&self.pool)
                .await?;
        let authors = sqlx::query("SELECT public_key, document_id, last_seen FROM authors")
            .fetch_all(&self.pool)
            .await?;

        let mut authors_per_document = authors.iter().fold(HashMap::new(), |mut acc, row| {
            let Ok(document_id) = row.try_get::<DocumentId, _>("document_id") else {
                return acc;
            };
            let Ok(public_key) = PublicKey::try_from(row.get::<&[u8], _>("public_key")) else {
                return acc;
            };
            let Ok(last_seen) = row.try_get::<Option<DateTime<Utc>>, _>("last_seen") else {
                return acc;
            };
            acc.entry(document_id)
                .or_insert_with(|| Vec::new())
                .push(Author {
                    public_key,
                    last_seen,
                });
            acc
        });

        for document in &mut documents {
            document.authors = authors_per_document
                .remove(&document.id)
                .expect("Document does not exist");
        }

        Ok(documents)
    }

    pub async fn add_document(&self, document_id: &DocumentId) -> sqlx::Result<()> {
        // The document_id is the primary key in the table therefore ignore insertion when the document exists already
        sqlx::query(
            "
            INSERT OR IGNORE INTO documents ( document_id )
            VALUES ( ? )
            ",
        )
        .bind(document_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn add_author(
        &self,
        document_id: &DocumentId,
        public_key: &PublicKey,
    ) -> sqlx::Result<()> {
        // The author/document_id pair is required to be unique therefore ignore if the insertion fails
        sqlx::query(
            "
            INSERT OR IGNORE INTO authors ( public_key, document_id )
            VALUES ( ?, ? )
            ",
        )
        .bind(public_key.as_bytes().as_slice())
        .bind(document_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_last_seen_for_author(
        &self,
        public_key: PublicKey,
        last_seen: Option<DateTime<Utc>>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE authors
            SET last_seen = ?
            WHERE public_key = ?
            ",
        )
        .bind(last_seen)
        .bind(public_key.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_name_for_document(
        &self,
        document_id: &DocumentId,
        name: Option<String>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE documents
            SET name = ?
            WHERE document_id = ?
            ",
        )
        .bind(name)
        .bind(document_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_last_accessed_for_document(
        &self,
        document_id: &DocumentId,
        last_accessed: Option<DateTime<Utc>>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE documents
            SET last_accessed = ?
            WHERE document_id = ?
            ",
        )
        .bind(last_accessed)
        .bind(document_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn operations_for_document(
        &self,
        operation_store: &OperationStore,
        document_id: &DocumentId,
    ) -> sqlx::Result<Vec<p2panda_core::Operation<ReflectionExtensions>>> {
        let operation_store = operation_store.inner();
        let authors = self.authors(document_id).await?;

        let log_ids = [
            LogId::new(LogType::Delta, document_id),
            LogId::new(LogType::Snapshot, document_id),
        ];

        let mut result = Vec::new();

        for author in authors.iter() {
            for log_id in &log_ids {
                let operations = match operation_store.get_log(author, log_id, None).await {
                    Ok(Some(operations)) => {
                        operations
                            .into_iter()
                            .map(|(header, body)| p2panda_core::Operation {
                                hash: header.hash(),
                                header,
                                body,
                            })
                    }
                    Ok(None) => {
                        continue;
                    }
                    Err(error) => {
                        error!(
                            "Failed to load operation for {author} with log type {log_id:?}: {error}"
                        );
                        continue;
                    }
                };

                result.extend(operations);
            }
        }

        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct LogId(LogType, DocumentId);

impl LogId {
    pub fn new(log_type: LogType, document: &DocumentId) -> Self {
        Self(log_type, *document)
    }
}

#[async_trait]
impl TopicLogMap<DocumentId, LogId> for DocumentStore {
    async fn get(&self, topic: &DocumentId) -> Option<HashMap<PublicKey, Vec<LogId>>> {
        let Ok(authors) = self.authors(topic).await else {
            return None;
        };
        let log_ids = [
            LogId::new(LogType::Delta, topic),
            LogId::new(LogType::Snapshot, topic),
        ];
        Some(
            authors
                .into_iter()
                .map(|author| (author, log_ids.to_vec()))
                .collect(),
        )
    }
}
