use std::hash::Hash as StdHash;

use anyhow::{Result, bail};
use p2panda_core::{Extension, Header, Operation, PruneFlag};
use p2panda_spaces::message::{AuthoredMessage, SpacesArgs, SpacesMessage};
use p2panda_spaces::types::Conditions;
use serde::{Deserialize, Serialize};

use crate::document::DocumentId;
use crate::store::LogId;

#[derive(Clone, Debug)]
pub struct ReflectionOperation(pub Operation<ReflectionExtensions>);

impl From<Operation<ReflectionExtensions>> for ReflectionOperation {
    fn from(value: Operation<ReflectionExtensions>) -> Self {
        Self(value)
    }
}

impl From<ReflectionOperation> for Operation<ReflectionExtensions> {
    fn from(value: ReflectionOperation) -> Self {
        value.0
    }
}

/// Custom extensions for p2panda header.
#[derive(Clone, Debug, Serialize, Deserialize)]
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

    /// Arguments required for interacting with `p2panda-spaces`.
    #[serde(rename = "s")]
    pub spaces_args: ReflectionSpacesArgs,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub enum LogType {
    // @TODO: We write everything into one log for now (system messages, snapshots and deltas, as
    // there's no message ordering in place, handling dependencies across logs).
    #[default]
    Spaces,
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
        // Document should always be set when needed.
        header
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.document)
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

#[derive(Clone, Debug, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct ReflectionConditions {}

impl Conditions for ReflectionConditions {}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReflectionSpacesArgs(SpacesArgs<ReflectionConditions>);

impl From<SpacesArgs<ReflectionConditions>> for ReflectionSpacesArgs {
    fn from(value: SpacesArgs<ReflectionConditions>) -> Self {
        Self(value)
    }
}

impl Extension<ReflectionSpacesArgs> for ReflectionExtensions {
    fn extract(header: &Header<Self>) -> Option<ReflectionSpacesArgs> {
        header.extension()
    }
}

impl AuthoredMessage for ReflectionOperation {
    fn id(&self) -> p2panda_spaces::types::OperationId {
        self.0.hash.into()
    }

    fn author(&self) -> p2panda_spaces::types::ActorId {
        self.0.header.public_key.into()
    }
}

impl SpacesMessage<ReflectionConditions> for ReflectionOperation {
    fn args(&self) -> &SpacesArgs<ReflectionConditions> {
        let extensions = self
            .0
            .header
            .extensions
            .as_ref()
            .expect("operations contain extensions");
        &extensions.spaces_args.0
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
