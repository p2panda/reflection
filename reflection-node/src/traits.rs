use std::sync::Arc;

use p2panda::VerifyingKey;
use p2panda::node::CreateStreamError;
use p2panda::processor::ProcessorError;
use p2panda::streams::{AckedError, DecodeError, ReplayError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubscriptionError {
    /// Broken / closed communication channel with the internal actor in `p2panda-net` prevented
    /// creation of stream. This can be due to the actor crashing.
    #[error(transparent)]
    CreateStream(#[from] CreateStreamError),

    /// Topic stream could not re-play events due to an internal error.
    #[error("{0}")]
    ReplayStream(Arc<ReplayError>),

    /// Acknowledgment of event failed due to critical error.
    #[error("{0}")]
    AckedMessage(Arc<AckedError>),

    /// Application payload could not be deserialized.
    #[error(transparent)]
    DecodeMessage(#[from] DecodeError),

    /// Operation failed during event processing of the system-level pipeline.
    #[error(transparent)]
    StreamProcessor(#[from] ProcessorError),
}

pub trait SubscribableTopic: Sync + Send {
    fn bytes_received(&self, author: VerifyingKey, data: Vec<u8>);
    fn ephemeral_bytes_received(&self, author: VerifyingKey, timestamp: u64, data: Vec<u8>);
    fn author_joined(&self, author: VerifyingKey);
    fn author_left(&self, author: VerifyingKey);
    fn error(&self, error: SubscriptionError);
}
