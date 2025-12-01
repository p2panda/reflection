use std::hash::Hash as StdHash;

use p2panda_core::{Extension, Header, PruneFlag};
use p2panda_net::TopicId;
use serde::{Deserialize, Serialize};

use crate::document_store::LogId;

/// Custom extensions for p2panda header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReflectionExtensions {
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

    /// Operations can be organised in separate logs. With a "log id" we can declare where this
    /// operation belongs to.
    ///
    /// We organise two logs per author per document, one for "short lived" / ephemeral deltas
    /// (small text changes) and one for persisted snapshots (full document history). These are two
    /// distinct "log types".
    #[serde(rename = "t")]
    pub log_type: LogType,

    /// Identifier of the document this operation relates to.
    #[serde(rename = "d")]
    pub document: TopicId,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub enum LogType {
    Snapshot,
    #[default]
    Delta,
}

impl Extension<PruneFlag> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<PruneFlag> {
        Some(header.extensions.prune_flag.clone())
    }
}

impl Extension<LogType> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<LogType> {
        Some(header.extensions.log_type)
    }
}

impl Extension<TopicId> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<TopicId> {
        Some(header.extensions.document)
    }
}

impl Extension<LogId> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<LogId> {
        let log_type: LogType = header.extension()?;
        let document_id: TopicId = header.extension()?;

        Some(LogId::new(log_type, &document_id))
    }
}
