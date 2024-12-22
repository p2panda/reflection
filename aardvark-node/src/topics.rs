use p2panda_core::{cbor, hash::HASH_LEN, Hash, PublicKey};
use p2panda_net::TopicId;
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};

use crate::document::ShortCode;

#[derive(Clone, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
pub struct DiscoveryCode(ShortCode);

impl DiscoveryCode {
    pub fn hash(&self) -> Hash {
        let short_code_str: String = self.0.iter().collect();
        Hash::new(short_code_str.as_bytes())
    }
}

impl TopicQuery for DiscoveryCode {}

impl TopicId for DiscoveryCode {
    fn id(&self) -> [u8; 32] {
        self.hash().as_bytes().to_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
pub struct TextDocument(pub PublicKey, pub u64);

impl TextDocument {
    pub fn hash(&self) -> Hash {
        let bytes = cbor::encode_cbor(self).expect("can encode as cbor bytes");
        Hash::new(bytes)
    }

    pub fn short_code(&self) -> ShortCode {
        let short_code = self.hash().to_hex().split_off(HASH_LEN - 6);
        let chars: Vec<char> = short_code.chars().collect();
        chars.try_into().unwrap()
    }
}

impl TopicQuery for TextDocument {}

impl TopicId for TextDocument {
    fn id(&self) -> [u8; 32] {
        self.hash().as_bytes().to_owned()
    }
}
