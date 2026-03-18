use p2panda::node::CreateStreamError;
use p2panda_core::PublicKey;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error(transparent)]
    CreateStream(#[from] CreateStreamError),
}

pub trait SubscribableTopic: Sync + Send {
    fn bytes_received(&self, author: PublicKey, data: Vec<u8>);
    fn author_joined(&self, author: PublicKey);
    fn author_left(&self, author: PublicKey);
    fn ephemeral_bytes_received(&self, author: PublicKey, data: Vec<u8>);
    fn error(&self, error: SubscriptionError);
}
