use std::path::PathBuf;
use std::pin::Pin;

use p2panda_store::sqlite::migrations as p2panda_migrations;
use sqlx::error::BoxDynError;
use sqlx::migrate::{Migration, MigrationSource, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use tracing::info;

/// Establishes a SQLite connection pool.
///
/// If no database path is given the database is created in memory.
pub async fn database_pool(db_file: Option<PathBuf>) -> Result<SqlitePool, sqlx::Error> {
    let connection_options = SqliteConnectOptions::new()
        .shared_cache(true)
        .create_if_missing(true);

    let pool = if let Some(db_file) = db_file {
        info!("database file location: {db_file:?}");
        let connection_options = connection_options.filename(db_file);
        SqlitePool::connect_with(connection_options).await?
    } else {
        let connection_options = connection_options.in_memory(true);
        // FIXME: we need to set max connection to 1 for in memory sqlite DB. Probably has to
        // do something with this issue: https://github.com/launchbadge/sqlx/issues/2510
        let pool_options = SqlitePoolOptions::new().max_connections(1);
        pool_options.connect_with(connection_options).await?
    };

    Ok(pool)
}

/// Run migration for p2panda and for the our TopicStore.
pub async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::migrate::MigrateError> {
    Migrator::new(CombinedMigrationSource::new(vec![
        p2panda_migrations(),
        sqlx::migrate!(),
    ]))
    .await?
    .run(pool)
    .await?;

    Ok(())
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Combine multiple `sqlx::migrate::Migrator` into a single `sqlx::migrate::MigrationSource`
///
/// See for more details: https://github.com/launchbadge/sqlx/discussions/3407
#[derive(Debug)]
pub struct CombinedMigrationSource {
    migrators: Vec<Migrator>,
}

impl CombinedMigrationSource {
    pub fn new(migrators: Vec<Migrator>) -> CombinedMigrationSource {
        Self { migrators }
    }
}

impl<'s> MigrationSource<'s> for CombinedMigrationSource {
    fn resolve(self) -> BoxFuture<'s, Result<Vec<Migration>, BoxDynError>> {
        Box::pin(async move {
            Ok(self
                .migrators
                .iter()
                .flat_map(|migrator| migrator.iter())
                .cloned()
                .collect())
        })
    }
}
