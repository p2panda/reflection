use serde::{Deserialize, Serialize};

use crate::author_tracker::AuthorTrackerMessage;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "t", content = "c")]
pub(crate) enum EphemeralMessage {
    /// Custom message to be forwarded to the application-layer.
    #[serde(rename = "app")]
    Application(Vec<u8>),

    /// Message used to track online status of authors.
    #[serde(rename = "author_tracker")]
    AuthorTracker(AuthorTrackerMessage),
}
