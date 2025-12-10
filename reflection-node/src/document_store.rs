use std::collections::HashMap;
use std::hash::Hash as StdHash;

use chrono::{DateTime, Utc};
use p2panda_core::PublicKey;
use p2panda_net::TopicId;
use p2panda_store::LogStore;
use p2panda_sync::{log_sync::Logs, topic_log_sync::TopicLogMap};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row};
use tracing::error;

use crate::operation::{LogType, ReflectionExtensions};
use crate::operation_store::OperationStore;

#[derive(Debug, FromRow)]
pub struct StoreDocument {
    #[sqlx(try_from = "Vec<u8>", rename = "document_id")]
    pub id: TopicId,
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

#[derive(Clone, Debug)]
pub struct DocumentStore {
    pool: sqlx::SqlitePool,
}

impl DocumentStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    async fn authors(&self, id: &TopicId) -> sqlx::Result<Vec<PublicKey>> {
        let list = sqlx::query("SELECT public_key FROM authors WHERE topic_id = ?")
            .bind(id.as_slice())
            .fetch_all(&self.pool)
            .await?;

        Ok(list
            .iter()
            .filter_map(|row| PublicKey::try_from(row.get::<&[u8], _>("public_key")).ok())
            .collect())
    }

    pub async fn documents(&self) -> sqlx::Result<Vec<StoreDocument>> {
        let mut documents: Vec<StoreDocument> =
            sqlx::query_as("SELECT id, name, last_accessed FROM documents")
                .fetch_all(&self.pool)
                .await?;
        let authors = sqlx::query("SELECT public_key, topic_id, last_seen FROM authors")
            .fetch_all(&self.pool)
            .await?;

        let mut authors_per_document = authors.iter().fold(HashMap::new(), |mut acc, row| {
            let Ok(id) = TopicId::try_from(row.get::<&[u8], _>("topic_id")) else {
                return acc;
            };
            let Ok(public_key) = PublicKey::try_from(row.get::<&[u8], _>("public_key")) else {
                return acc;
            };
            let Ok(last_seen) = row.try_get::<Option<DateTime<Utc>>, _>("last_seen") else {
                return acc;
            };
            acc.entry(id).or_insert_with(Vec::new).push(Author {
                public_key,
                last_seen,
            });
            acc
        });

        for document in &mut documents {
            if let Some(authors) = authors_per_document.remove(&document.id) {
                document.authors = authors;
            }
        }

        Ok(documents)
    }

    pub async fn add_document(&self, id: &TopicId) -> sqlx::Result<()> {
        // The id is the primary key in the table therefore ignore insertion when the document exists already
        sqlx::query(
            "
            INSERT OR IGNORE INTO documents ( id )
            VALUES ( ? )
            ",
        )
        .bind(id.as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_document(&self, id: &TopicId) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM documents WHERE id = ?")
            .bind(id.as_slice())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn add_author(&self, id: &TopicId, public_key: &PublicKey) -> sqlx::Result<()> {
        // The author/id pair is required to be unique therefore ignore if the insertion fails
        sqlx::query(
            "
            INSERT OR IGNORE INTO authors ( public_key, topic_id )
            VALUES ( ?, ? )
            ",
        )
        .bind(public_key.as_bytes().as_slice())
        .bind(id.as_slice())
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
        id: &TopicId,
        name: Option<String>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE documents
            SET name = ?
            WHERE id = ?
            ",
        )
        .bind(name)
        .bind(id.as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_last_accessed_for_document(
        &self,
        id: &TopicId,
        last_accessed: Option<DateTime<Utc>>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE documents
            SET last_accessed = ?
            WHERE id = ?
            ",
        )
        .bind(last_accessed)
        .bind(id.as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn operations_for_document(
        &self,
        operation_store: &OperationStore,
        id: &TopicId,
    ) -> sqlx::Result<Vec<p2panda_core::Operation<ReflectionExtensions>>> {
        let operation_store = operation_store.inner();
        let authors = self.authors(id).await?;

        let log_ids = [
            LogId::new(LogType::Delta, id),
            LogId::new(LogType::Snapshot, id),
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
pub struct LogId(LogType, TopicId);

impl LogId {
    pub fn new(log_type: LogType, document: &TopicId) -> Self {
        Self(log_type, *document)
    }
}

impl TopicLogMap<TopicId, LogId> for DocumentStore {
    type Error = sqlx::Error;

    async fn get(&self, topic: &TopicId) -> Result<Logs<LogId>, Self::Error> {
        let authors = self.authors(topic).await?;

        let log_ids = [
            LogId::new(LogType::Delta, topic),
            LogId::new(LogType::Snapshot, topic),
        ];
        Ok(authors
            .into_iter()
            .map(|author| (author, log_ids.to_vec()))
            .collect())
    }
}
