use p2panda_core::{
    Body, Header, Operation,
    cbor::{DecodeError, decode_cbor},
};
use thiserror::Error;

use crate::document::DocumentId;
use crate::operation::ReflectionExtensions;

type OperationWithRawHeader = (Header<ReflectionExtensions>, Option<Body>, Vec<u8>);

#[derive(Debug, Error)]
pub enum UnpackError {
    #[error(transparent)]
    Cbor(#[from] DecodeError),
    #[error("Operation with invalid document id")]
    InvalidDocumentId,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct PersistentOperation {
    #[serde(with = "serde_bytes")]
    header: Vec<u8>,
    body: Option<serde_bytes::ByteBuf>,
}

impl PersistentOperation {
    pub fn new(operation: Operation<ReflectionExtensions>) -> Self {
        Self {
            header: operation.header.to_bytes(),
            body: operation.body.map(|body| body.to_bytes().into()),
        }
    }

    pub fn from_serialized(header: Vec<u8>, body: Option<Vec<u8>>) -> Self {
        Self {
            header,
            body: body.map(Into::into),
        }
    }

    /// Validates and unpacks the operation
    pub fn validate_and_unpack(
        self,
        id: DocumentId,
    ) -> Result<OperationWithRawHeader, UnpackError> {
        let PersistentOperation { header, body } = self;

        // The header is serialized by Header::to_bytes() as cbor
        let header_deserialized: Header<ReflectionExtensions> = decode_cbor(&header[..])?;
        let body_deserialized = body.map(|body| Body::from(body.into_vec()));

        let Some(operation_id): Option<DocumentId> = header_deserialized.extension()
        else {
            return Err(UnpackError::InvalidDocumentId);
        };

        if operation_id != id {
            return Err(UnpackError::InvalidDocumentId);
        }

        Ok((header_deserialized, body_deserialized, header))
    }
}
