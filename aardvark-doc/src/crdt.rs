use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use loro::event::{Diff, DiffEvent};
use loro::{EventTriggerKind, ExportMode, LoroDoc, Subscription};

/// Identifier in container where we store the text in the Loro document.
const TEXT_CONTAINER_ID: &str = "document";

pub type EventReceiver = async_channel::Receiver<TextCrdtEvent>;

pub struct TextCrdt {
    doc: RefCell<LoroDoc>,
    event_rx: EventReceiver,
    #[allow(dead_code)]
    subscription: Subscription,
    #[allow(dead_code)]
    subscription_local: Subscription,
}

impl TextCrdt {
    pub fn new(peer_id: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_record_timestamp(false);
        doc.set_peer_id(peer_id)
            .expect("set peer id for new document");

        let text = doc.get_text(TEXT_CONTAINER_ID);

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

    pub fn from_bytes(peer_id: u64, bytes: &[u8]) -> Result<Self> {
        let crdt = Self::new(peer_id);
        {
            let inner = crdt.doc.borrow_mut();
            inner.import_with(bytes, "snapshot")?;
        }
        Ok(crdt)
    }

    pub fn subscribe(&mut self) -> EventReceiver {
        self.event_rx.clone()
    }

    pub fn insert(&mut self, index: usize, chunk: &str) -> Result<()> {
        let doc = self.doc.get_mut();
        let text = doc.get_text(TEXT_CONTAINER_ID);
        text.insert(index, chunk)?;
        doc.commit();
        Ok(())
    }

    pub fn remove(&mut self, index: usize, len: usize) -> Result<()> {
        let doc = self.doc.get_mut();
        let text = doc.get_text(TEXT_CONTAINER_ID);
        text.delete(index, len)?;
        doc.commit();
        Ok(())
    }

    pub fn apply_encoded_delta(&mut self, bytes: &[u8]) -> Result<()> {
        let doc = self.doc.get_mut();
        doc.import_with(bytes, "delta")?;
        Ok(())
    }

    pub fn apply_delta(&mut self, delta: TextDelta) -> Result<()> {
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

    pub fn snapshot(&self) -> Vec<u8> {
        let doc = self.doc.borrow();
        doc.export(ExportMode::Snapshot)
            .expect("encoded crdt snapshot")
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

#[derive(Debug)]
pub enum TextCrdtEvent {
    Local(TextDelta),
    LocalEncoded(Vec<u8>),
    Remote(Vec<TextDelta>),
}

impl TextCrdtEvent {
    fn from_deltas(triggered_by: loro::EventTriggerKind, mut deltas: Vec<TextDelta>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::{TextCrdt, TextCrdtEvent};

    #[test]
    fn from_snapshot() {
        let mut doc_1 = TextCrdt::new(1);

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
        let mut doc_1 = TextCrdt::new(1);
        let rx_1 = doc_1.subscribe();

        doc_1.insert(0, "Hello,").unwrap();
        doc_1.insert(6, " World!").unwrap();
        doc_1.remove(7, 1).unwrap();
        doc_1.insert(7, "W").unwrap();

        let mut doc_2 = TextCrdt::new(2);

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
        let mut doc_1 = TextCrdt::new(1);
        let rx_1 = doc_1.subscribe();

        doc_1.insert(0, "Hello").unwrap();
        doc_1.remove(1, 4).unwrap();
        doc_1.insert(1, "uhu!").unwrap();

        assert_eq!(doc_1.to_string(), "Huhu!");

        let mut doc_2 = TextCrdt::new(2);

        for _ in 0..6 {
            if let TextCrdtEvent::Local(delta) = rx_1.recv().await.unwrap() {
                doc_2.apply_delta(delta).unwrap();
            }
        }

        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }
}
