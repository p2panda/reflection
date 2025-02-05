use std::time::SystemTime;

use anyhow::Result;
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use p2panda_core::{Body, Extension, Extensions, Header, PrivateKey, PruneFlag};
use p2panda_store::{LogStore, MemoryStore};
use p2panda_stream::operation::ingest_operation;
use serde::{Deserialize, Serialize};

use crate::topic::TextDocument;

/// Custom extensions for p2panda header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AardvarkExtensions {
    /// If flag is true we can remove all previous operations in this log.
    ///
    /// This usually indicates that a "snapshot" has been inserted into the body of this operation,
    /// containing all required state to reconstruct the full version including all previous edits
    /// of this document.
    ///
    /// In our case of a text-editor, this would be the encoded payload of a state-based CRDT.
    #[serde(
        rename = "p",
        skip_serializing_if = "PruneFlag::is_not_set",
        default = "PruneFlag::default"
    )]
    pub prune_flag: PruneFlag,

    /// Identifier of the text document this operation relates to.
    ///
    /// Can be `None` if this operation indicates that we are creating a new document. In this case
    /// we take the hash of the header itself to derive the document id.
    #[serde(rename = "d")]
    pub document: Option<TextDocument>,
}

impl Extension<PruneFlag> for AardvarkExtensions {
    fn extract(header: &Header<Self>) -> Option<PruneFlag> {
        header
            .extensions
            .as_ref()
            .map(|extensions| extensions.prune_flag.clone())
    }
}

impl Extension<TextDocument> for AardvarkExtensions {
    fn extract(header: &Header<Self>) -> Option<TextDocument> {
        // If this is the first operation in the append-only log we use the hash of the header
        // itself to determine the document id, otherwise use the one mentioned in the header by
        // subsequent operations.
        match header.seq_num {
            0 => Some(header.hash().into()),
            _ => header
                .extensions
                .as_ref()
                .map(|extensions| extensions.document.clone())
                .flatten(),
        }
    }
}

/// Creates, signs and stores new operation in the author's append-only log.
///
/// We maintain one log per author and document. If no document is specified we create a new
/// operation in a new log. The resulting hash of the header can be used to identify that new
/// document.
pub async fn create_operation(
    store: &mut MemoryStore<TextDocument, AardvarkExtensions>,
    private_key: &PrivateKey,
    document: Option<TextDocument>,
    body: Option<&[u8]>,
    prune_flag: bool,
) -> Result<(Header<AardvarkExtensions>, Option<Body>)> {
    let body = body.map(Body::new);

    let public_key = private_key.public_key();

    let latest_operation = match document {
        Some(ref document) => store.latest_operation(&public_key, &document).await?,
        None => None,
    };

    let (seq_num, backlink) = match latest_operation {
        Some((header, _)) => (header.seq_num + 1, Some(header.hash())),
        None => (0, None),
    };

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();

    let extensions = AardvarkExtensions {
        prune_flag: PruneFlag::new(prune_flag),
        document: document.clone(),
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

    let log_id: TextDocument = header.extension().expect("document id from our own logs");
    let prune_flag: PruneFlag = header.extension().unwrap_or_default();
    ingest_operation(
        store,
        header.clone(),
        body.clone(),
        header.to_bytes(),
        &log_id,
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
