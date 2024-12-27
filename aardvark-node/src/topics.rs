use p2panda_core::{cbor, Hash, PublicKey};
use p2panda_net::TopicId;
use p2panda_sync::TopicQuery;
use serde::{Deserialize, Serialize};

use crate::document::ShortCode;

#[derive(Clone, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
pub enum AardvarkTopics {
    DiscoveryCode(DiscoveryCode),
    TextDocument(TextDocument),
}

impl TopicQuery for AardvarkTopics {}

impl TopicId for AardvarkTopics {
    fn id(&self) -> [u8; 32] {
        match self {
            AardvarkTopics::DiscoveryCode(discovery_code) => {
                discovery_code.hash().as_bytes().to_owned()
            }
            AardvarkTopics::TextDocument(text_document) => {
                text_document.hash().as_bytes().to_owned()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, std::hash::Hash, Serialize, Deserialize)]
pub struct DiscoveryCode(pub ShortCode);

impl DiscoveryCode {
    pub fn hash(&self) -> Hash {
        let short_code_str: String = self.0.iter().collect();
        Hash::new(short_code_str.as_bytes())
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
        let mut hex_hash = self.hash().to_hex();
        let short_code = hex_hash.split_off(hex_hash.len() - 6);
        let chars: Vec<char> = short_code.chars().collect();
        chars.try_into().unwrap()
    }
}
