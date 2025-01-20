use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use loro::event::{Diff, DiffEvent};
use loro::{EventTriggerKind, ExportMode, LoroDoc, Subscription, TextDelta};

/// Identifier in container where we store the text in the Loro document.
const TEXT_CONTAINER_ID: &str = "document";

pub type UpdateReceiver = async_channel::Receiver<DocumentUpdate>;

pub struct Document {
    doc: RefCell<LoroDoc>,
    update_rx: UpdateReceiver,
    #[allow(dead_code)]
    subscription: Subscription,
    #[allow(dead_code)]
    subscription_local: Subscription,
}

impl Document {
    pub fn new(peer_id: u64) -> Self {
        let doc = LoroDoc::new();
        doc.set_record_timestamp(false);
        doc.set_peer_id(peer_id)
            .expect("set peer id for new document");

        let text = doc.get_text(TEXT_CONTAINER_ID);

        let (update_tx, update_rx) = async_channel::bounded::<DocumentUpdate>(64);

        let subscription = {
            let update_tx = update_tx.clone();
            doc.subscribe(
                &text.id(),
                Arc::new(move |event| {
                    let update = quill_delta_to_update(event);
                    let _ = update_tx.send_blocking(update);
                }),
            )
        };

        let subscription_local = doc.subscribe_local_update(Box::new(move |bytes| {
            let _ = update_tx.send_blocking(DocumentUpdate::LocalEncoded(bytes.to_owned()));
            true
        }));

        Self {
            doc: RefCell::new(doc),
            update_rx,
            subscription,
            subscription_local,
        }
    }

    pub fn from_bytes(peer_id: u64, bytes: &[u8]) -> Result<Self> {
        let doc = Self::new(peer_id);
        {
            let inner = doc.doc.borrow_mut();
            inner.import_with(bytes, "snapshot")?;
        }
        Ok(doc)
    }

    pub fn subscribe(&mut self) -> UpdateReceiver {
        self.update_rx.clone()
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

    pub fn apply_delta(&mut self, delta: Delta) -> Result<()> {
        match delta {
            Delta::Insert { index, chunk } => {
                self.insert(index, &chunk)?;
            }
            Delta::Remove { index, len } => {
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

impl fmt::Display for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let doc = self.doc.borrow();
        let text = doc.get_text(TEXT_CONTAINER_ID);
        write!(f, "{}", text.to_string())
    }
}

#[derive(Clone, Debug)]
pub enum Delta {
    Insert { index: usize, chunk: String },
    Remove { index: usize, len: usize },
}

#[derive(Debug)]
pub enum DocumentUpdate {
    Local(Delta),
    LocalEncoded(Vec<u8>),
    Remote(Vec<Delta>),
}

fn quill_delta_to_update(diff_event: DiffEvent<'_>) -> DocumentUpdate {
    let mut deltas = Vec::new();
    let mut index = 0;

    for event in diff_event.events {
        if event.is_unknown {
            continue;
        }

        match event.diff {
            Diff::Text(quill_deltas) => {
                for quill in quill_deltas {
                    let delta = match quill {
                        TextDelta::Retain { retain, .. } => {
                            index += retain;
                            continue;
                        }
                        TextDelta::Insert { insert, .. } => Delta::Insert {
                            index,
                            chunk: insert,
                        },
                        TextDelta::Delete { delete } => Delta::Remove { index, len: delete },
                    };
                    deltas.push(delta);
                }
            }
            _ => continue,
        }
    }

    match diff_event.triggered_by {
        EventTriggerKind::Local => {
            assert_eq!(deltas.len(), 1, "local updates have exactly one delta");
            DocumentUpdate::Local(deltas.get(0).unwrap().clone())
        }
        EventTriggerKind::Import => DocumentUpdate::Remote(deltas),
        EventTriggerKind::Checkout => unimplemented!("checkouts not supported currently"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Document, DocumentUpdate};

    #[test]
    fn from_snapshot() {
        let mut doc_1 = Document::new(1);

        doc_1.insert(0, "Hello,").unwrap();
        doc_1.insert(6, " World!").unwrap();
        doc_1.remove(7, 1).unwrap();
        doc_1.insert(7, "W").unwrap();

        let doc_2 = Document::from_bytes(2, &doc_1.snapshot()).unwrap();

        assert_eq!(doc_1.to_string(), "Hello, World!");
        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }

    #[tokio::test]
    async fn from_encoded_deltas() {
        let mut doc_1 = Document::new(1);
        let rx_1 = doc_1.subscribe();

        doc_1.insert(0, "Hello,").unwrap();
        doc_1.insert(6, " World!").unwrap();
        doc_1.remove(7, 1).unwrap();
        doc_1.insert(7, "W").unwrap();

        let mut doc_2 = Document::new(2);

        for _ in 0..8 {
            if let DocumentUpdate::LocalEncoded(bytes) = rx_1.recv().await.unwrap() {
                doc_2.apply_encoded_delta(&bytes).unwrap();
            }
        }

        assert_eq!(doc_1.to_string(), "Hello, World!");
        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }

    #[tokio::test]
    async fn from_deltas() {
        let mut doc_1 = Document::new(1);
        let rx_1 = doc_1.subscribe();

        doc_1.insert(0, "Hello").unwrap();
        doc_1.remove(1, 4).unwrap();
        doc_1.insert(1, "uhu!").unwrap();

        assert_eq!(doc_1.to_string(), "Huhu!");

        let mut doc_2 = Document::new(2);

        for _ in 0..6 {
            if let DocumentUpdate::Local(delta) = rx_1.recv().await.unwrap() {
                doc_2.apply_delta(delta).unwrap();
            }
        }

        assert_eq!(doc_1.to_string(), doc_2.to_string());
    }
}
