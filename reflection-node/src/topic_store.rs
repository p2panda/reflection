use std::collections::HashMap;

use chrono::{DateTime, Utc};
use p2panda::{Topic, VerifyingKey};
use sqlx::{FromRow, Row};

#[derive(Debug, FromRow)]
pub struct TopicRow {
    #[sqlx(try_from = "Vec<u8>")]
    pub id: Topic,
    #[sqlx(default)]
    pub name: Option<String>,
    pub last_accessed: Option<DateTime<Utc>>,
    #[sqlx(skip)]
    pub authors: Vec<TrackedAuthor>,
}

#[derive(Debug, Clone)]
pub struct TrackedAuthor {
    pub verifying_key: VerifyingKey,
    pub last_seen: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct TrackedTopicStore {
    pool: sqlx::SqlitePool,
}

impl TrackedTopicStore {
    pub fn from_pool(pool: sqlx::SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn topics(&self) -> sqlx::Result<Vec<TopicRow>> {
        let mut topics: Vec<TopicRow> =
            sqlx::query_as("SELECT id, name, last_accessed FROM topics")
                .fetch_all(&self.pool)
                .await?;
        let authors = sqlx::query("SELECT verifying_key, topic_id, last_seen FROM authors")
            .fetch_all(&self.pool)
            .await?;

        let mut authors_per_topic = authors.iter().fold(HashMap::new(), |mut acc, row| {
            let Ok(id) = Topic::try_from(row.get::<&[u8], _>("topic_id")) else {
                return acc;
            };
            let Ok(verifying_key) = VerifyingKey::try_from(row.get::<&[u8], _>("verifying_key"))
            else {
                return acc;
            };
            let Ok(last_seen) = row.try_get::<Option<DateTime<Utc>>, _>("last_seen") else {
                return acc;
            };
            acc.entry(id).or_insert_with(Vec::new).push(TrackedAuthor {
                verifying_key,
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

    pub async fn add_author(
        &self,
        topic: &Topic,
        verifying_key: &VerifyingKey,
    ) -> sqlx::Result<()> {
        // The author/id pair is required to be unique therefore ignore if the insertion fails
        sqlx::query(
            "
            INSERT OR IGNORE INTO authors ( verifying_key, topic_id )
            VALUES ( ?, ? )
            ",
        )
        .bind(verifying_key.as_bytes().as_slice())
        .bind(topic.as_bytes().as_slice())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn set_last_seen_for_author(
        &self,
        topic: Topic,
        verifying_key: VerifyingKey,
        last_seen: Option<DateTime<Utc>>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "
            UPDATE authors
            SET
                last_seen = ?
            WHERE
                verifying_key = ?
                AND topic_id = ?
            ",
        )
        .bind(last_seen)
        .bind(verifying_key.as_bytes().as_slice())
        .bind(topic.as_bytes().as_slice())
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

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use p2panda::{SigningKey, Topic};

    use crate::database::{database_pool, run_migrations};

    use super::TrackedTopicStore;

    #[tokio::test]
    async fn last_seen_is_per_topic() {
        let pool = database_pool(None).await.unwrap();
        run_migrations(&pool).await.unwrap();
        let store = TrackedTopicStore::from_pool(pool);

        let author = SigningKey::generate().verifying_key();
        let (topic_a, topic_b) = (Topic::from([1; 32]), Topic::from([2; 32]));
        for topic in [&topic_a, &topic_b] {
            store.add_topic(topic).await.unwrap();
            store.add_author(topic, &author).await.unwrap();
        }

        store
            .set_last_seen_for_author(topic_a, author, Some(Utc::now()))
            .await
            .unwrap();

        let topics = store.topics().await.unwrap();
        let last_seen = |topic: &Topic| {
            let row = topics.iter().find(|row| &row.id == topic).unwrap();
            row.authors[0].last_seen
        };
        assert!(last_seen(&topic_a).is_some());
        assert!(last_seen(&topic_b).is_none());
    }
}
