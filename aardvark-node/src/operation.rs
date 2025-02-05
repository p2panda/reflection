use std::time::SystemTime;

use anyhow::Result;
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use p2panda_core::{Body, Extension, Extensions, Header, PrivateKey, PruneFlag};
use p2panda_store::{LogStore, MemoryStore};
use p2panda_stream::operation::ingest_operation;
use serde::{Deserialize, Serialize};

use crate::topic::TextDocument;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AardvarkExtensions {
    #[serde(
        rename = "p",
        skip_serializing_if = "PruneFlag::is_not_set",
        default = "PruneFlag::default"
    )]
    pub prune_flag: PruneFlag,

    #[serde(rename = "d")]
    pub document_id: TextDocument,
}

impl Extension<PruneFlag> for AardvarkExtensions {
    fn extract(&self) -> Option<PruneFlag> {
        Some(self.prune_flag.clone())
    }
}

impl Extension<TextDocument> for AardvarkExtensions {
    fn extract(&self) -> Option<TextDocument> {
        Some(self.document_id.clone())
    }
}

pub async fn create_operation(
    store: &mut MemoryStore<TextDocument, AardvarkExtensions>,
    private_key: &PrivateKey,
    document_id: TextDocument,
    body: Option<&[u8]>,
    prune_flag: bool,
) -> Result<(Header<AardvarkExtensions>, Option<Body>)> {
    let body = body.map(Body::new);

    let public_key = private_key.public_key();

    let latest_operation = store.latest_operation(&public_key, &document_id).await?;

    let (seq_num, backlink) = match latest_operation {
        Some((header, _)) => (header.seq_num + 1, Some(header.hash())),
        None => (0, None),
    };

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();

    let extensions = AardvarkExtensions {
        prune_flag: PruneFlag::new(prune_flag),
        document_id: document_id.clone(),
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
    header.sign(private_key);

    let prune_flag: PruneFlag = header.extract().unwrap_or_default();
    ingest_operation(
        store,
        header.clone(),
        body.clone(),
        header.to_bytes(),
        &document_id,
        prune_flag.is_set(),
    )
    .await?;

    Ok((header, body))
}

pub fn encode_gossip_operation<E>(header: Header<E>, body: Option<Body>) -> Result<Vec<u8>>
where
    E: Extensions + Serialize,
{
    let bytes = encode_cbor(&(header.to_bytes(), body.map(|body| body.to_bytes())))?;
    Ok(bytes)
}

pub fn decode_gossip_message(bytes: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let result = decode_cbor(bytes)?;
    Ok(result)
}
