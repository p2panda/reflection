use std::collections::HashMap;
use std::hash::Hash as StdHash;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use p2panda_core::PublicKey;
use p2panda_store::{LogStore, SqliteStore};
use p2panda_sync::log_sync::TopicLogMap;
use serde::{Deserialize, Serialize};
use sqlx;
use sqlx::Row;
use tracing::error;

use crate::document::{Author, Document, DocumentId};
use crate::operation::{AardvarkExtensions, LogType, validate_operation};

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

    pub async fn documents(&self) -> sqlx::Result<Vec<Document>> {
        let mut documents: Vec<Document> =
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
    ) -> sqlx::Result<Vec<p2panda_core::Operation<AardvarkExtensions>>> {
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
                        operations.into_iter().map(|(header, body)| {
                            let operation = p2panda_core::Operation {
                                hash: header.hash(),
                                header,
                                body,
                            };

                            // Stored operations are always valid
                            assert!(validate_operation(&operation, &document_id).is_ok());
                            operation
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

pub type OperationStore = SqliteStore<LogId, AardvarkExtensions>;
