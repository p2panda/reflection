use std::sync::Mutex;

use gio::prelude::*;
use gio::subclass::prelude::ListModelImpl;
use glib::subclass::prelude::*;

use crate::author::Author;
use crate::identity::PublicKey;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Authors {
        pub list: Mutex<Vec<Author>>,
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

    pub(crate) fn from_vec(authors: Vec<Author>) -> Self {
        let obj: Self = glib::Object::new();
        *obj.imp().list.lock().unwrap() = authors;
        obj
    }

    pub(crate) fn add_this_device(&self, author_key: PublicKey) {
        let mut list = self.imp().list.lock().unwrap();
        let pos = list.len() as u32;

        let author = Author::for_this_device(&author_key);
        list.push(author);
        drop(list);
        self.items_changed(pos, 0, 1);
    }

    pub(crate) fn ensure_author(&self, author_key: PublicKey) {
        let mut list = self.imp().list.lock().unwrap();

        if !list.iter().any(|author| author.public_key() == author_key) {
            let pos = list.len() as u32;

            let author = Author::new(&author_key);

            list.push(author);
            drop(list);

            self.items_changed(pos, 0, 1);
        }
    }

    pub(crate) fn add_or_update(&self, author_key: PublicKey, is_online: bool) {
        let mut list = self.imp().list.lock().unwrap();

        if let Some(author) = list.iter().find(|author| author.public_key() == author_key) {
            author.set_is_online(is_online);
        } else {
            let pos = list.len() as u32;

            let author = Author::new(&author_key);

            list.push(author);
            drop(list);

            self.items_changed(pos, 0, 1);
        }
    }

    pub(crate) fn author(&self, author_key: PublicKey) -> Option<Author> {
        let list = self.imp().list.lock().unwrap();
        list.iter()
            .find(|author| author.public_key() == author_key)
            .cloned()
    }
}
