use std::sync::Mutex;

use gio::prelude::*;
use gio::subclass::prelude::ListModelImpl;
use glib::{clone, subclass::prelude::*};

use crate::document::{Document, DocumentId};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Documents {
        pub(super) list: Mutex<Vec<Document>>,
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
            self.list.lock().unwrap().len() as u32
        }

        fn item(&self, index: u32) -> Option<glib::Object> {
            self.list
                .lock()
                .unwrap()
                .get(index as usize)
                .cloned()
                .map(Cast::upcast)
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
        glib::source::idle_add_full(
            glib::source::Priority::DEFAULT,
            clone!(
                #[weak(rename_to = obj)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    let mut list = obj.imp().list.lock().unwrap();
                    let pos = list.len() as u32;

                    list.push(document.clone());
                    drop(list);
                    obj.items_changed(pos, 0, 1);
                    glib::ControlFlow::Break
                }
            ),
        );
    }

    pub fn by_id(&self, document_id: &DocumentId) -> Option<Document> {
        let list = self.imp().list.lock().unwrap();

        list.iter()
            .find(|document| &document.id() == document_id)
            .cloned()
    }
}
