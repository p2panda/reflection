use p2panda_core::{
    Body, Header, Operation,
    cbor::{DecodeError, decode_cbor},
};

use crate::operation::ReflectionExtensions;

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

    /// Unpacks the operation
    pub fn unpack(
        self,
    ) -> Result<(Header<ReflectionExtensions>, Option<Body>, Vec<u8>), DecodeError> {
        let PersistentOperation { header, body } = self;

        // The header is serialized by Header::to_bytes() as cbor
        let header_deserialized = decode_cbor(&header[..])?;
        let body_deserialized = body.map(|body| Body::from(body.into_vec()));

        Ok((header_deserialized, body_deserialized, header))
    }
}
