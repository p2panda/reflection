use std::cell::RefCell;
use std::fmt;

use anyhow::Result;
use automerge::transaction::Transactable;
use automerge::{AutoCommit, AutoSerde, ObjId, ObjType, Patch, ReadDoc};

/// Hard-coded automerge document schema in bytes representation for "Aardvark".
///
/// Creating a local document based on this schema allows peers to independently do so as they'll
/// all have the same schema and object ids in the end. Otherwise peers wouldn't be able to merge
/// their changes into each other's documents as the id's wouldn't match.
///
/// Read more here:
/// <https://automerge.org/docs/cookbook/modeling-data/#setting-up-an-initial-document-structure>
const DOCUMENT_SCHEMA: [u8; 119] = [
    133, 111, 74, 131, 14, 200, 8, 95, 0, 109, 1, 16, 163, 64, 79, 49, 42, 30, 77, 109, 146, 45,
    91, 5, 214, 2, 217, 205, 1, 252, 203, 208, 39, 6, 89, 188, 223, 101, 41, 50, 160, 144, 47, 147,
    187, 74, 77, 252, 185, 64, 18, 211, 205, 23, 118, 97, 221, 216, 176, 1, 239, 6, 1, 2, 3, 2, 19,
    2, 35, 2, 64, 2, 86, 2, 7, 21, 5, 33, 2, 35, 2, 52, 1, 66, 2, 86, 2, 128, 1, 2, 127, 0, 127, 1,
    127, 1, 127, 0, 127, 0, 127, 7, 127, 3, 100, 111, 99, 127, 0, 127, 1, 1, 127, 4, 127, 0, 127,
    0, 0,
];

/// Identifier in automerge document path where we store the text.
const DOCUMENT_OBJ_ID: &str = "doc";

pub struct Document {
    doc: RefCell<AutoCommit>,
}

impl Document {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let mut doc = AutoCommit::new();
        doc.put_object(automerge::ROOT, DOCUMENT_OBJ_ID, ObjType::Text)
            .expect("inserting text object at root");
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

    fn text_object(&self) -> ObjId {
        let doc = self.doc.borrow();
        let (_value, obj_id) = doc
            .get(automerge::ROOT, DOCUMENT_OBJ_ID)
            .unwrap_or_default()
            .expect("text object at root");
        obj_id
    }

    pub fn update(&self, position: i32, del: i32, text: &str) -> Result<()> {
        let text_obj = self.text_object();
        let mut doc = self.doc.borrow_mut();
        doc.splice_text(&text_obj, position as usize, del as isize, text)?;
        // Move the diff pointer forward to current position
        doc.update_diff_cursor();
        Ok(())
    }

    pub fn load_incremental(&self, bytes: &[u8]) -> Result<()> {
        let mut doc = self.doc.borrow_mut();
        doc.load_incremental(&bytes)?;
        Ok(())
    }

    pub fn diff_incremental(&self) -> Vec<Patch> {
        let mut doc = self.doc.borrow_mut();
        doc.diff_incremental()
    }

    pub fn text(&self) -> String {
        let text_obj = self.text_object();
        let doc = self.doc.borrow();
        doc.text(&text_obj)
            .expect("text to be given in automerge document")
    }

    #[allow(dead_code)]
    pub fn save(&self) -> Vec<u8> {
        let mut doc = self.doc.borrow_mut();
        doc.save()
    }

    pub fn save_incremental(&self) -> Vec<u8> {
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
        let doc = self.doc.borrow();
        let json = serde_json::to_string_pretty(&AutoSerde::from(&*doc))
            .expect("serialize automerge document to JSON");
        write!(f, "{}", json)
    }
}
