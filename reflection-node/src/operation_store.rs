use std::sync::Arc;
use std::time::{SystemTime, SystemTimeError};

use crate::document_store::LogId;
use p2panda_core::{Body, Header, Operation, PrivateKey, PruneFlag};
use p2panda_net::TopicId;
use p2panda_store::{
    LogStore, OperationStore as TraitOperationStore, SqliteStore, SqliteStoreError,
};
use thiserror::Error;
use tokio::sync::Semaphore;

use crate::operation::{LogType, ReflectionExtensions};

#[derive(Debug, Error)]
pub enum CreationError {
    #[error(transparent)]
    SytemTime(#[from] SystemTimeError),
    #[error(transparent)]
    Store(#[from] SqliteStoreError),
}

#[derive(Debug)]
pub struct OperationStore {
    inner: SqliteStore<LogId, ReflectionExtensions>,
    // FIXME: This makes sure we only create one operation at the time and not in parallel
    // Since we would mess up the sequence of operations
    semaphore_operation_store: Arc<Semaphore>,
}

impl OperationStore {
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            inner: SqliteStore::new(pool),
            semaphore_operation_store: Arc::new(Semaphore::new(1)),
        }
    }

    pub fn clone_inner(&self) -> SqliteStore<LogId, ReflectionExtensions> {
        self.inner.clone()
    }

    pub fn inner(&self) -> &SqliteStore<LogId, ReflectionExtensions> {
        &self.inner
    }

    /// Creates, signs and stores new operation in the author's append-only log.
    ///
    /// If no document is specified we create a new operation in a new log. The resulting hash of the
    /// header can be used to identify that new document.
    pub async fn create_operation(
        &self,
        private_key: &PrivateKey,
        log_type: LogType,
        document: TopicId,
        body: Option<&[u8]>,
        prune_flag: bool,
    ) -> Result<Operation<ReflectionExtensions>, CreationError> {
        let _permit = self
            .semaphore_operation_store
            .acquire()
            .await
            .expect("OperationStore semaphore not to be closed");

        let body = body.map(Body::new);
        let public_key = private_key.public_key();

        let log_id = LogId::new(log_type, &document);
        let latest_operation = self.inner.latest_operation(&public_key, &log_id).await?;

        let (seq_num, backlink) = match latest_operation {
            Some((header, _)) => (header.seq_num + 1, Some(header.hash())),
            None => (0, None),
        };

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();

        let extensions = ReflectionExtensions {
            prune_flag: PruneFlag::new(prune_flag),
            log_type,
            document,
        };

        let mut header = Header {
            version: 1,
            public_key,
            signature: None,
            payload_size: body.as_ref().map_or(0, |body| body.size()),
            payload_hash: body.as_ref().map(|body| body.hash()),
            timestamp,
            seq_num,
            backlink,
            previous: vec![],
            extensions,
        };
        header.sign(private_key);

        let operation = Operation {
            hash: header.hash(),
            header,
            body,
        };

        let mut inner_clone = self.clone_inner();
        inner_clone
            .insert_operation(
                operation.hash,
                &operation.header,
                operation.body.as_ref(),
                operation.header.to_bytes().as_slice(),
                &log_id,
            )
            .await?;

        if prune_flag {
            inner_clone
                .delete_operations(
                    &operation.header.public_key,
                    &log_id,
                    operation.header.seq_num,
                )
                .await?;
        }

        Ok(operation)
    }
}
