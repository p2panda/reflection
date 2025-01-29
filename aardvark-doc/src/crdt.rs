use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use loro::event::{Diff, DiffEvent};
use loro::{EventTriggerKind, ExportMode, LoroDoc, Subscription};
use thiserror::Error;

/// Identifier of container where we handle the text CRDT in a Loro document.
///
/// Loro documents can contain multiple different CRDT types in one document. We can address these
/// with identifiers.
const TEXT_CONTAINER_ID: &str = "document";

pub type EventReceiver = async_channel::Receiver<TextCrdtEvent>;

/// Manages a Conflict-free Replicated Data Type (CRDTs) to resolve parallel edits by multiple
/// authors on the same text document.
///
/// Internally this uses a text CRDT implementation by [Loro](https://www.loro.dev/). This
/// interface serves merely as a wrapper to bring Loro and it's data into the shape we need,
/// without worrying too much about the internal details of Loro.
pub struct TextCrdt {
    doc: RefCell<LoroDoc>,
    event_rx: EventReceiver,
    #[allow(dead_code)]
    subscription: Subscription,
    #[allow(dead_code)]
    subscription_local: Subscription,
}

impl TextCrdt {
    /// Returns new instance managing a text CRDT.
    ///
    /// Use this when creating a new document.
    pub fn new(peer_id: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_record_timestamp(false);
        doc.set_peer_id(peer_id)
            .expect("set peer id for new document");

        let text = doc.get_text(TEXT_CONTAINER_ID);

        // NOTE(adz): We're introducing a non-tokio channel implementation here as using a tokio
        // channel would cause a panic in this setup.
        //
        // Tokio (rightly) informs us about using a `send_blocking` inside the same thread where
        // the async runtime operates, thus potentially blocking it.
        //
        // This is not optimal but seems to work for now, later we might want to look into running
        // the whole CRDT logic in a separate thread.
        let (event_tx, event_rx) = async_channel::bounded::<TextCrdtEvent>(64);

        let subscription = {
            let event_tx = event_tx.clone();
            doc.subscribe(
                &text.id(),
                Arc::new(move |loro_event| {
                    let triggered_by = loro_event.triggered_by;
                    let deltas = {
                        let loro_deltas = extract_text_deltas(loro_event);
                        absolute_deltas(loro_deltas)
                    };
                    let event = TextCrdtEvent::from_deltas(triggered_by, deltas);
                    let _ = event_tx.send_blocking(event);
                }),
            )
        };

        let subscription_local = doc.subscribe_local_update(Box::new(move |bytes| {
            let _ = event_tx.send_blocking(TextCrdtEvent::LocalEncoded(bytes.to_owned()));
            true
        }));

        Self {
            doc: RefCell::new(doc),
            event_rx,
            subscription,
            subscription_local,
        }
    }

    /// Returns text CRDT instance from a snapshot.
    ///
    /// Use this when restoring an existing, local document (for example when it was stored on your
    /// file system) or when receiving a full snapshot from another peer after joining an existing
    /// document.
    pub fn from_bytes(peer_id: u64, bytes: &[u8]) -> Result<Self, TextCrdtError> {
        let crdt = Self::new(peer_id);
        {
            let inner = crdt.doc.borrow_mut();
            inner
                .import_with(bytes, "snapshot")
                .map_err(|err| TextCrdtError::Imported(err))?;
        }
        Ok(crdt)
    }

    /// Subscribe to changes to the document.
    ///
    /// This should be used as the "source of truth" for all text operations (local and remote text
    /// inserts and removals), affecting all "higher layer" state (text buffer).
    ///
    /// ## Local Changes
    ///
    /// ```text
    /// -> User types something
    ///     -> Text CRDT "insert" or "removal" called
    ///         -> Commit & create "local" delta event
    ///             -> Delta Event received via subscription
    ///                 -> Apply delta to text buffer
    /// ```
    ///
    /// ## Remote Changes
    ///
    /// ```text
    /// -> Received deltas from remote peer (via networking layer)
    ///     -> Apply encoded delta to Text CRDT
    ///         -> Commit & create "remote" delta event
    ///             -> Delta Event received via subscription
    ///                 -> Apply delta to text buffer
    /// ```
    pub fn subscribe(&self) -> EventReceiver {
        self.event_rx.clone()
    }

    /// Inserts text at the given unicode position.
    ///
    /// This text change gets directly committed, causing a local "delta event" which should be
    /// used to update "higher layer" state, like the text buffer. Read
    /// [subscribe](#method.subscribe) for receiving and handling these events.
    pub fn insert(&self, index: usize, chunk: &str) -> Result<(), TextCrdtError> {
        let doc = self.doc.borrow_mut();
        let text = doc.get_text(TEXT_CONTAINER_ID);
        text.insert(index, chunk)
            .map_err(|err| TextCrdtError::Local(err))?;
        doc.commit();
        Ok(())
    }

    /// Removes range of text at the given unicode position with unicode length.
    ///
    /// This text change gets directly committed, causing a local "delta event" which should be
    /// used to update "higher layer" state, like the text buffer. Read
    /// [subscribe](#method.subscribe) for receiving and handling these events.
    pub fn remove(&self, index: usize, len: usize) -> Result<(), TextCrdtError> {
        let doc = self.doc.borrow_mut();
        let text = doc.get_text(TEXT_CONTAINER_ID);
        text.delete(index, len)
            .map_err(|err| TextCrdtError::Local(err))?;
        doc.commit();
        Ok(())
    }

    /// Applies encoded text deltas received from a remote peer.
    ///
    /// Deltas are encoded according to the Loro specification.
    pub fn apply_encoded_delta(&self, bytes: &[u8]) -> Result<(), TextCrdtError> {
        let doc = self.doc.borrow_mut();
        doc.import_with(bytes, "delta")
            .map_err(|err| TextCrdtError::Imported(err))?;
        Ok(())
    }

    /// Exports encoded snapshot of current Text CRDT state.
    ///
    /// This can be used to persist the current state of the text CRDT on the file system or during
    /// initial sync when a remote peer joins our document. See [from_bytes](#method.from_bytes)
    /// for the reverse method.
    ///
    /// Snapshots are encoded according to the Loro specification.
    pub fn snapshot(&self) -> Vec<u8> {
        let doc = self.doc.borrow();
        doc.export(ExportMode::Snapshot)
            .expect("encoded crdt snapshot")
    }

    /// Applies local text changes.
    #[cfg(test)]
    fn apply_delta(&self, delta: TextDelta) -> Result<(), TextCrdtError> {
        match delta {
            TextDelta::Insert { index, chunk } => {
                self.insert(index, &chunk)?;
            }
            TextDelta::Remove { index, len } => {
                self.remove(index, len)?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for TextCrdt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let doc = self.doc.borrow();
        let text = doc.get_text(TEXT_CONTAINER_ID);
        write!(f, "{}", text.to_string())
    }
}

#[derive(Clone, Debug)]
pub enum TextDelta {
    Insert { index: usize, chunk: String },
    Remove { index: usize, len: usize },
}

/// Events to notify other parts of the application about text changes.
#[derive(Debug)]
pub enum TextCrdtEvent {
    /// We've locally inserted or removed text.
    ///
    /// Use this to apply changes to your local text buffer, etc.
    Local(TextDelta),

    /// Same as `Local` but in encoded form, including additional information like a vector clock,
    /// so we can send that delta over the wire to other peers.
    ///
    /// Use this to send "small" text changes directly to other peers, for example via gossip
    /// broadcast.
    LocalEncoded(Vec<u8>),

    /// Remote peer inserted or removed text.
    ///
    /// If a snapshot was received (for example during initial sync), this event might contain
    /// multiple deltas.
    ///
    /// Use this to apply remote changes to your local text buffer.
    Remote(Vec<TextDelta>),
}

impl TextCrdtEvent {
    fn from_deltas(triggered_by: EventTriggerKind, mut deltas: Vec<TextDelta>) -> Self {
        match triggered_by {
            EventTriggerKind::Local => {
                // Since we're committing inserts and removals directly on local changes, we can assure
                // that there's only one delta given. As soon as we're changing the commit logic we
                // need to revisit this.
                assert_eq!(deltas.len(), 1, "local updates have exactly one delta");
                Self::Local(deltas.pop().expect("one delta"))
            }
            EventTriggerKind::Import => Self::Remote(deltas),
            EventTriggerKind::Checkout => unimplemented!("document checkouts are not supported"),
        }
    }
}

/// Loro supports all sorts of CRDTs (Lists, Maps, Counters, etc.), this method extracts only the
/// deltas related to collaborative text editing of our known text container.
fn extract_text_deltas(diff_event: DiffEvent<'_>) -> Vec<loro::TextDelta> {
    diff_event
        .events
        .into_iter()
        .filter_map(|event| {
            if event.is_unknown {
                return None;
            }

            if let Diff::Text(loro_deltas) = event.diff {
                Some(loro_deltas)
            } else {
                None
            }
        })
        .flatten()
        .collect()
}

/// Converts relative text deltas to absolute ones.
///
/// Loro's text deltas are represented as QuillJS "Deltas" which encode text inserts and removals
/// relative to position 0 in the document.
///
/// For our purposes we need absolute positions, as our text buffer implementation requires the
/// exact position for every text insertion and removal.
///
/// Read more: https://quilljs.com/docs/delta/
fn absolute_deltas(loro_deltas: Vec<loro::TextDelta>) -> Vec<TextDelta> {
    let mut deltas = Vec::new();
    let mut index = 0;

    for loro_delta in loro_deltas {
        let delta = match loro_delta {
            loro::TextDelta::Retain { retain, .. } => {
                index += retain;
                continue;
            }
            loro::TextDelta::Insert { insert, .. } => TextDelta::Insert {
                index,
                chunk: insert,
            },
            loro::TextDelta::Delete { delete } => TextDelta::Remove { index, len: delete },
        };
        deltas.push(delta);
    }

    deltas
}

#[derive(Debug, Error)]
pub enum TextCrdtError {
    #[error("could not apply local text change: {0}")]
    Local(loro::LoroError),

    #[error("could not apply imported text change: {0}")]
    Imported(loro::LoroError),
}

#[cfg(test)]
mod tests {
    use super::{TextCrdt, TextCrdtEvent};

    #[test]
    fn from_snapshot() {
        let doc_1 = TextCrdt::new(1);

        doc_1.insert(0, "Hello,").unwrap();
        doc_1.insert(6, " World!").unwrap();
        doc_1.remove(7, 1).unwrap();
        doc_1.insert(7, "W").unwrap();

        let doc_2 = TextCrdt::from_bytes(2, &doc_1.snapshot()).unwrap();

        assert_eq!(doc_1.to_string(), "Hello, World!");
        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }

    #[tokio::test]
    async fn from_encoded_deltas() {
        let doc_1 = TextCrdt::new(1);
        let rx_1 = doc_1.subscribe();

        doc_1.insert(0, "Hello,").unwrap();
        doc_1.insert(6, " World!").unwrap();
        doc_1.remove(7, 1).unwrap();
        doc_1.insert(7, "W").unwrap();

        let doc_2 = TextCrdt::new(2);

        for _ in 0..8 {
            if let TextCrdtEvent::LocalEncoded(bytes) = rx_1.recv().await.unwrap() {
                doc_2.apply_encoded_delta(&bytes).unwrap();
            }
        }

        assert_eq!(doc_1.to_string(), "Hello, World!");
        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }

    #[tokio::test]
    async fn from_deltas() {
        let doc_1 = TextCrdt::new(1);
        let rx_1 = doc_1.subscribe();

        doc_1.insert(0, "Hello").unwrap();
        doc_1.remove(1, 4).unwrap();
        doc_1.insert(1, "uhu!").unwrap();

        assert_eq!(doc_1.to_string(), "Huhu!");

        let doc_2 = TextCrdt::new(2);

        for _ in 0..6 {
            if let TextCrdtEvent::Local(delta) = rx_1.recv().await.unwrap() {
                doc_2.apply_delta(delta).unwrap();
            }
        }

        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }
}
