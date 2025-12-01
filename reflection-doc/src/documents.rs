use std::sync::RwLock;

use gio::prelude::*;
use gio::subclass::prelude::ListModelImpl;
use glib::subclass::prelude::*;
use indexmap::IndexMap;

use crate::identity::PublicKey;
use crate::service::StartupError;
use crate::{
    author::Author,
    document::{Document, DocumentId},
    service::Service,
};

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

    pub(crate) async fn load(&self, service: &Service) -> Result<(), StartupError> {
        let public_key = service.private_key().public_key();

        let documents = service.node().documents::<DocumentId>().await?;

        let mut list = self.imp().list.write().unwrap();
        assert!(list.is_empty());

        let documents_len = documents.len();
        for document in documents {
            let last_accessed = document.last_accessed.and_then(|last_accessed| {
                glib::DateTime::from_unix_utc(last_accessed.timestamp()).ok()
            });

            let authors: Vec<Author> = document
                .authors
                .iter()
                .map(|author| {
                    let author_public_key = PublicKey(author.public_key);
                    if author_public_key == public_key {
                        let last_seen = author.last_seen.and_then(|last_seen| {
                            glib::DateTime::from_unix_utc(last_seen.timestamp()).ok()
                        });
                        Author::for_this_device(&PublicKey(author.public_key), last_seen.as_ref())
                    } else {
                        let last_seen = author.last_seen.and_then(|last_seen| {
                            glib::DateTime::from_unix_utc(last_seen.timestamp()).ok()
                        });
                        Author::with_state(&author_public_key, last_seen.as_ref())
                    }
                })
                .collect();

            let obj = Document::with_state(
                service,
                Some(&document.id),
                document.name.as_deref(),
                last_accessed.as_ref(),
            );

            obj.authors().load(authors);

            list.insert(document.id, obj);
        }

        drop(list);
        self.items_changed(0, 0, documents_len as u32);

        Ok(())
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

    pub fn remove(&self, document_id: &DocumentId) {
        let mut list = self.imp().list.write().unwrap();
        let list_len = list.len();

        if let Some((index, _, _)) = list.shift_remove_full(document_id) {
            drop(list);
            self.items_changed((list_len - index - 1) as u32, 1, 0);
        }
    }

    pub fn document(&self, document_id: &DocumentId) -> Option<Document> {
        let list = self.imp().list.read().unwrap();

        list.get(document_id).cloned()
    }
}
