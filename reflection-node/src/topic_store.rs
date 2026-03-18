use std::collections::HashMap;

use chrono::{DateTime, Utc};
use p2panda_core::{PublicKey, Topic};
use sqlx::{FromRow, Row};

#[derive(Debug, FromRow)]
pub struct StoreTopic {
    #[sqlx(try_from = "Vec<u8>")]
    pub id: Topic,
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
pub struct TopicStore {
    pool: sqlx::SqlitePool,
}

impl TopicStore {
    pub fn from_pool(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn topics(&self) -> sqlx::Result<Vec<StoreTopic>> {
        let mut topics: Vec<StoreTopic> =
            sqlx::query_as("SELECT id, name, last_accessed FROM topics")
                .fetch_all(&self.pool)
                .await?;
        let authors = sqlx::query("SELECT public_key, topic_id, last_seen FROM authors")
            .fetch_all(&self.pool)
            .await?;

        let mut authors_per_topic = authors.iter().fold(HashMap::new(), |mut acc, row| {
            let Ok(id) = Topic::try_from(row.get::<&[u8], _>("topic_id")) else {
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

        for topic in &mut topics {
            if let Some(authors) = authors_per_topic.remove(&topic.id) {
                topic.authors = authors;
            }
        }

        Ok(topics)
    }

    pub async fn add_topic(&self, topic: &Topic) -> sqlx::Result<()> {
        // The id is the primary key in the table therefore ignore insertion when the topic exists
        // already
        sqlx::query(
            "
            INSERT OR IGNORE INTO topics ( id )
            VALUES ( ? )
            ",
        )
        .bind(topic.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_topic(&self, topic: &Topic) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM topics WHERE id = ?")
            .bind(topic.as_bytes().as_slice())
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn add_author(&self, topic: &Topic, public_key: &PublicKey) -> sqlx::Result<()> {
        // The author/id pair is required to be unique therefore ignore if the insertion fails
        sqlx::query(
            "
            INSERT OR IGNORE INTO authors ( public_key, topic_id )
            VALUES ( ?, ? )
            ",
        )
        .bind(public_key.as_bytes().as_slice())
        .bind(topic.as_bytes().as_slice())
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

    pub async fn set_name_for_topic(
        &self,
        topic: &Topic,
        name: Option<String>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE topics
            SET name = ?
            WHERE id = ?
            ",
        )
        .bind(name)
        .bind(topic.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_last_accessed_for_topic(
        &self,
        topic: &Topic,
        last_accessed: Option<DateTime<Utc>>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE topics
            SET last_accessed = ?
            WHERE id = ?
            ",
        )
        .bind(last_accessed)
        .bind(topic.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
