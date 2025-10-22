use std::sync::RwLock;

use gio::prelude::*;
use gio::subclass::prelude::ListModelImpl;
use glib::subclass::prelude::*;
use indexmap::IndexMap;

use crate::author::Author;
use crate::identity::PublicKey;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Authors {
        pub(super) list: RwLock<IndexMap<PublicKey, Author>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Authors {
        const NAME: &'static str = "Authors";
        type Type = super::Authors;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for Authors {}

    impl ListModelImpl for Authors {
        fn item_type(&self) -> glib::Type {
            Author::static_type()
        }

        fn n_items(&self) -> u32 {
            self.list.read().unwrap().len() as u32
        }

        fn item(&self, index: u32) -> Option<glib::Object> {
            let list = self.list.read().unwrap();
            list.get_index(index as usize)
                .map(|(_, v)| v.upcast_ref::<glib::Object>())
                .cloned()
        }
    }
}

glib::wrapper! {
    pub struct Authors(ObjectSubclass<imp::Authors>)
    @implements gio::ListModel;
}

unsafe impl Send for Authors {}
unsafe impl Sync for Authors {}

impl Default for Authors {
    fn default() -> Self {
        Self::new()
    }
}

impl Authors {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub(crate) fn load(&self, authors: Vec<Author>) {
        let mut list = self.imp().list.write().unwrap();
        let authors_len = authors.len();

        // Only this device should be in the list
        assert_eq!(list.len(), 1);

        for author in authors {
            let public_key = author.public_key();
            if !list.contains_key(&public_key) {
                list.insert(public_key, author);
            }
        }

        drop(list);
        self.items_changed(1, 0, authors_len as u32);
    }

    pub(crate) fn add_this_device(&self, author_key: PublicKey) {
        let mut list = self.imp().list.write().unwrap();
        let now = glib::DateTime::now_local().ok();

        assert!(list.is_empty());

        let author = Author::for_this_device(&author_key, now.as_ref());
        list.insert(author_key, author);
        drop(list);
        self.items_changed(0, 0, 1);
    }

    pub(crate) fn add(&self, author_key: PublicKey) -> Author {
        let mut list = self.imp().list.write().unwrap();
        list.entry(author_key)
            .or_insert_with_key(|key| Author::new(&key))
            .to_owned()
    }

    pub(crate) fn author(&self, author_key: &PublicKey) -> Option<Author> {
        let list = self.imp().list.read().unwrap();
        list.get(author_key).cloned()
    }
}
