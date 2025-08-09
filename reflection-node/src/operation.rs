use std::hash::Hash as StdHash;

use anyhow::{Result, bail};
use p2panda_core::{Extension, Header, Operation, PruneFlag};
use serde::{Deserialize, Serialize};

use crate::document::DocumentId;
use crate::store::LogId;

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

    /// Identifier of the text document this operation relates to.
    ///
    /// Can be `None` if this operation indicates that we are creating a new document. In this case
    /// we take the hash of the header itself to derive the document id.
    #[serde(rename = "d")]
    pub document: Option<DocumentId>,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub enum LogType {
    Snapshot,
    #[default]
    Delta,
}

impl Extension<PruneFlag> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<PruneFlag> {
        header
            .extensions
            .as_ref()
            .map(|extensions| extensions.prune_flag.clone())
    }
}

impl Extension<LogType> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<LogType> {
        header
            .extensions
            .as_ref()
            .map(|extensions| extensions.log_type)
    }
}

impl Extension<DocumentId> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<DocumentId> {
        // Check if header mentions an document.
        let document = header
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.document);
        if document.is_some() {
            return document;
        }

        // No document was mentioned, we must be creating a new document.
        //
        // If this is the first operation in the append-only log we use the hash of the header
        // itself to determine the document id.
        //
        // Subsequent operations will continue to mention it, if this is not the case we have an
        // invalid operation. In this case we return `None` here and our validation logic will
        // fail.
        match header.seq_num {
            0 => Some(header.hash().into()),
            _ => header
                .extensions
                .as_ref()
                .and_then(|extensions| extensions.document),
        }
    }
}

impl Extension<LogId> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<LogId> {
        let log_type: Option<LogType> = header.extension();
        let document: Option<DocumentId> = header.extension();

        if let (Some(log_type), Some(document)) = (log_type, document) {
            Some(LogId::new(log_type, &document))
        } else {
            None
        }
    }
}

/// Custom validation for our own operation headers.
pub fn validate_operation(
    operation: &Operation<ReflectionExtensions>,
    expected_document: &DocumentId,
) -> Result<()> {
    let given_document: Option<DocumentId> = operation.header.extension();
    match given_document {
        Some(given_document) => {
            if &given_document != expected_document {
                bail!(
                    "document id mismatch (expected: {}, received: {})",
                    expected_document,
                    given_document
                );
            }
        }
        None => {
            bail!("document id missing (expected: {})", expected_document);
        }
    }
    Ok(())
}
