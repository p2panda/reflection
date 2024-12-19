use std::time::SystemTime;

use anyhow::Result;
use p2panda_core::{Body, Extension, Extensions, Hash, Header, PrivateKey, PruneFlag};
use p2panda_store::{LogStore, MemoryStore};
use p2panda_stream::operation::ingest_operation;
use serde::{Deserialize, Serialize};

use crate::network::TextDocument;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AardvarkExtensions {
    #[serde(rename = "p", skip_serializing_if = "Option::is_none")]
    pub prune_flag: Option<PruneFlag>,

    #[serde(rename = "d", skip_serializing_if = "Option::is_none")]
    pub document_id: Option<TextDocument>,
}

impl Extension<PruneFlag> for AardvarkExtensions {
    fn extract(&self) -> Option<PruneFlag> {
        self.prune_flag.clone()
    }
}

impl Extension<TextDocument> for AardvarkExtensions {
    fn extract(&self) -> Option<TextDocument> {
        self.document_id.clone()
    }
}

pub fn encode_gossip_operation<E>(header: Header<E>, body: Option<Body>) -> Result<Vec<u8>>
where
    E: Extensions + Serialize,
{
    let mut bytes = Vec::new();
    ciborium::into_writer(
        &(header.to_bytes(), body.map(|body| body.to_bytes())),
        &mut bytes,
    )?;
    Ok(bytes)
}

pub fn decode_gossip_message(bytes: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let raw_operation = ciborium::from_reader(bytes)?;
    Ok(raw_operation)
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
        prune_flag: Some(PruneFlag::new(prune_flag)),
        document_id: Some(document_id.clone()),
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

pub async fn init_document(
    store: &mut MemoryStore<TextDocument, AardvarkExtensions>,
    private_key: &PrivateKey,
    body: Vec<u8>,
) -> Result<TextDocument> {
    let body = Body::new(&body);
    let public_key = private_key.public_key();

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("can get system time")
        .as_secs();

    let document_id = Hash::new(format!("{}-{}", private_key.public_key(), timestamp).as_bytes());
    let document_id = TextDocument(document_id.to_string());

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();

    let extensions = AardvarkExtensions {
        prune_flag: None,
        document_id: Some(document_id.clone()),
    };

    let mut header = Header {
        version: 1,
        public_key,
        signature: None,
        payload_size: body.size(),
        payload_hash: Some(body.hash()),
        timestamp,
        seq_num: 0,
        backlink: None,
        previous: vec![],
        extensions: Some(extensions),
    };
    header.sign(private_key);

    ingest_operation(
        store,
        header.clone(),
        Some(body),
        header.to_bytes(),
        &document_id,
        false,
    )
    .await?;

    Ok(document_id)
}
