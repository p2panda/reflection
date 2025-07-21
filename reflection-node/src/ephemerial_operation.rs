use p2panda_core::identity::{PrivateKey, PublicKey, Signature};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EphemerialOperation {
    #[serde(with = "serde_bytes")]
    body: Vec<u8>,
    author: PublicKey,
    signature: Signature,
}

impl EphemerialOperation {
    pub fn new(body: Vec<u8>, author: &PrivateKey) -> Self {
        Self {
            signature: author.sign(&body),
            body,
            author: author.public_key(),
        }
    }

    /// Validates the signature and unpacks the operation
    pub fn validate_and_unpack(self) -> Option<(PublicKey, Vec<u8>)> {
        let EphemerialOperation {
            body,
            author,
            signature,
        } = self;

        if self.author.verify(&body[..], &signature) {
            Some((author, body))
        } else {
            None
        }
    }
}
