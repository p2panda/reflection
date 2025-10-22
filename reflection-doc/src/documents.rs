use std::sync::RwLock;

use gio::prelude::*;
use gio::subclass::prelude::ListModelImpl;
use glib::subclass::prelude::*;
use indexmap::IndexMap;

use crate::document::{Document, DocumentId};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Documents {
        pub(super) list: RwLock<IndexMap<DocumentId, Document>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Documents {
        const NAME: &'static str = "Documents";
        type Type = super::Documents;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for Documents {}

    impl ListModelImpl for Documents {
        fn item_type(&self) -> glib::Type {
            Document::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.read().unwrap().len() as u32
        }

        fn item(&self, index: u32) -> Option<glib::Object> {
            let list = self.list.read().unwrap();
            list.get_index(list.len() - index as usize - 1)
                .map(|(_, v)| v.upcast_ref::<glib::Object>())
                .cloned()
        }
    }
}

glib::wrapper! {
    pub struct Documents(ObjectSubclass<imp::Documents>)
    @implements gio::ListModel;
}

unsafe impl Send for Documents {}
unsafe impl Sync for Documents {}

impl Default for Documents {
    fn default() -> Self {
        Self::new()
    }
}

impl Documents {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn add(&self, document: Document) {
        let mut list = self.imp().list.write().unwrap();
        let document_id = document.id();

        if list.contains_key(&document_id) {
            return;
        }

        list.insert(document_id, document);
        drop(list);
        self.items_changed(0, 0, 1);
    }

    pub fn document(&self, document_id: &DocumentId) -> Option<Document> {
        let list = self.imp().list.read().unwrap();

        list.get(document_id).cloned()
    }
}
