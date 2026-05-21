use p2panda::VerifyingKey;
use p2panda::node::CreateStreamError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error(transparent)]
    CreateStream(#[from] CreateStreamError),
}

pub trait SubscribableTopic: Sync + Send {
    fn bytes_received(&self, author: VerifyingKey, data: Vec<u8>);
    fn ephemeral_bytes_received(&self, author: VerifyingKey, timestamp: u64, data: Vec<u8>);
    fn author_joined(&self, author: VerifyingKey);
    fn author_left(&self, author: VerifyingKey);
    fn error(&self, error: SubscriptionError);
}
