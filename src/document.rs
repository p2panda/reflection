use std::fmt;

use anyhow::Result;
use automerge::transaction::Transactable;
use automerge::{AutoCommit, AutoSerde, Patch};

/// Hard-coded automerge document schema in bytes representation for "Aardvark".
///
/// Creating a local document based on this schema allows peers to independently do so as they'll
/// all have the same schema and object ids in the end. Otherwise peers wouldn't be able to merge
/// their changes into each other's documents as the id's wouldn't match.
///
/// Read more here:
/// <https://automerge.org/docs/cookbook/modeling-data/#setting-up-an-initial-document-structure>
const DOCUMENT_SCHEMA: [u8] = [1, 2, 3];

/// Identifier in automerge document path where we store the text.
const DOCUMENT_OBJ_ID: &str = "doc";

#[derive(Debug)]
pub struct Document {
    doc: RefCell<AutoCommit>,
}

impl Document {
    pub fn new() -> Self {
        let doc = AutoCommit::new();
        doc.put_object(automerge::ROOT, DOCUMENT_OBJ_ID, ObjType::Text)
            .expect("inserting text object '{DOCUMENT_OBJ_ID}' at root");
        Self {
            doc: RefCell::new(doc),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        let doc = AutoCommit::load(bytes).expect("load automerge document from bytes");
        Self {
            doc: RefCell::new(doc),
        }
    }

    pub fn update(&mut self, position: i32, del: i32, text: &str) -> Result<()> {
        let mut doc = self.doc.borrow_mut();
        doc.splice_text(&root, position as usize, del as isize, text)?;
        // Move the diff pointer forward to current position
        doc.update_diff_cursor();
        Ok(())
    }

    pub fn load_incremental(&mut self, bytes: &[u8]) -> Result<()> {
        let mut doc = self.doc.borrow_mut();
        doc.load_incremental(&bytes)?;
        Ok(())
    }

    pub fn diff_incremental(&mut self) -> Vec<Patch> {
        let mut doc = self.doc.borrow_mut();
        doc.diff_incremental()
    }

    pub fn text(&self) -> String {
        let doc = self.doc.borrow();
        let obj = doc.get(automerge::ROOT, DOCUMENT_OBJ_ID);
        doc.text(&obj)
            .expect("text to be given in automerge document")
    }

    pub fn save(&mut self) -> Vec<u8> {
        let mut doc = self.doc.borrow_mut();
        doc.save()
    }

    pub fn save_incremental(&mut self) -> Vec<u8> {
        let mut doc = self.doc.borrow_mut();
        doc.save_incremental()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::from_bytes(&DOCUMENT_SCHEMA)
    }
}

impl fmt::Debug for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut doc = self.doc.borrow();
        let json = serde_json::to_string_pretty(&AutoSerde::from(doc))
            .expect("serialize automerge document to JSON");
        write!(f, "{}", json)
    }
}
