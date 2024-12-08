use std::time::SystemTime;

use anyhow::Result;
use p2panda_core::{Body, Extension, Header, Operation, PrivateKey, PruneFlag};
use p2panda_store::{LogStore, MemoryStore, OperationStore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::network::TextDocument;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct AardvarkExtensions {
    #[serde(rename = "p", skip_serializing_if = "Option::is_none")]
    pub prune_flag: Option<PruneFlag>,
}

impl Extension<PruneFlag> for AardvarkExtensions {
    fn extract(&self) -> Option<PruneFlag> {
        self.prune_flag.clone()
    }
}

pub fn encode_operation<E>(header: Header<E>, body: Option<Body>) -> Result<Vec<u8>>
where
    E: Clone + Serialize,
{
    let operation: (Header<E>, Option<Body>) = (header, body);
    let mut bytes = Vec::new();
    ciborium::into_writer(&operation, &mut bytes)?;
    Ok(bytes)
}

pub fn decode_operation<E>(bytes: &[u8]) -> Result<(Header<E>, Option<Body>)>
where
    E: DeserializeOwned,
{
    let raw_operation = ciborium::from_reader(bytes)?;
    Ok(raw_operation)
}

pub async fn create_operation(
    store: &mut MemoryStore<TextDocument, AardvarkExtensions>,
    private_key: &PrivateKey,
    log_id: TextDocument,
    body: Option<&[u8]>,
    prune_flag: bool,
) -> Result<(Header<AardvarkExtensions>, Option<Body>)> {
    let body = body.map(Body::new);

    let public_key = private_key.public_key();

    let latest_operation = store.latest_operation(&public_key, &log_id).await?;

    let (seq_num, backlink) = match latest_operation {
        Some((header, body)) => (header.seq_num + 1, Some(header.hash())),
        None => (0, None),
    };

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();

    let extensions = AardvarkExtensions {
        prune_flag: Some(PruneFlag::new(prune_flag)),
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

    store
        .insert_operation(
            header.hash(),
            &header,
            body.as_ref(),
            &header.to_bytes(),
            &log_id,
        )
        .await?;

    if prune_flag {
        assert!(
            header.seq_num > 0,
            "can't prune from first operation in log"
        );
        store
            .delete_operations(&header.public_key, &log_id, header.seq_num)
            .await?;
    }

    Ok((header, body))
}
