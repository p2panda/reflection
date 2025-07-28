use std::time::{SystemTime, SystemTimeError};

use p2panda_core::{Body, Header, Operation, PrivateKey, PruneFlag, PublicKey};
use p2panda_spaces::{forge::Forge, message::SpacesArgs};
use p2panda_store::{LogStore, OperationStore as OperationStoreTrait, SqliteStoreError};
use thiserror::Error;

use crate::document::DocumentId;
use crate::operation::{LogType, ReflectionConditions, ReflectionExtensions, ReflectionOperation};
use crate::store::{LogId, OperationStore};

#[derive(Debug)]
pub struct ReflectionForge {
    operation_store: OperationStore,
    private_key: PrivateKey,
}

impl ReflectionForge {
    pub fn new(private_key: PrivateKey, operation_store: OperationStore) -> Self {
        Self {
            private_key,
            operation_store,
        }
    }
}

impl Forge<ReflectionOperation, ReflectionConditions> for ReflectionForge {
    type Error = ForgeError;

    fn public_key(&self) -> PublicKey {
        self.private_key.public_key()
    }

    async fn forge(
        &mut self,
        args: SpacesArgs<ReflectionConditions>,
    ) -> Result<ReflectionOperation, Self::Error> {
        let body = {
            if let SpacesArgs::Application { ciphertext, .. } = &args {
                Some(Body::new(ciphertext))
            } else {
                None
            }
        };

        let (document_id, log_type) = match args {
            SpacesArgs::KeyBundle {} => unimplemented!(),
            SpacesArgs::ControlMessage { id, .. } => (id.into(), LogType::Spaces),
            // @TODO: There is no way to tell the forge from the outside which
            // application message type this is (snapshot or delta), for now we
            // assume all application messages are snapshots here.
            SpacesArgs::Application { space_id, .. } => (space_id.into(), LogType::Snapshot),
        };

        let public_key = self.private_key.public_key();

        let latest_operation = {
            let log_id = LogId::new(log_type, &document_id);
            self.operation_store
                .latest_operation(&public_key, &log_id)
                .await?
        };

        let (seq_num, backlink) = match latest_operation {
            Some((header, _)) => (header.seq_num + 1, Some(header.hash())),
            None => (0, None),
        };

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs();

        let extensions = ReflectionExtensions {
            prune_flag: PruneFlag::new(false),
            log_type: log_type,
            document: Some(document_id),
            spaces_args: Some(args.into()),
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
            extensions: Some(extensions),
        };
        header.sign(&self.private_key);

        let document: DocumentId = header.extension().expect("document id from our own logs");
        let log_id = LogId::new(log_type, &document);

        let operation = Operation {
            hash: header.hash(),
            header,
            body,
        };

        self.operation_store
            .insert_operation(
                operation.hash,
                &operation.header,
                operation.body.as_ref(),
                operation.header.to_bytes().as_slice(),
                &log_id,
            )
            .await?;

        Ok(operation.into())
    }

    async fn forge_ephemeral(
        &mut self,
        private_key: PrivateKey,
        args: SpacesArgs<ReflectionConditions>,
    ) -> Result<ReflectionOperation, Self::Error> {
        unimplemented!()
    }
}

#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
pub enum ForgeError {
    #[error(transparent)]
    OperationStore(#[from] SqliteStoreError),

    #[error(transparent)]
    SystemTime(#[from] SystemTimeError),
}
