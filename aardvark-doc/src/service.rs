use glib::subclass::prelude::*;
use p2panda_core::{Hash, PrivateKey, PublicKey};
use tokio::sync::mpsc;
use tracing::info;

use aardvark_node::Network;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Service {
        pub network: Network,
        pub private_key: PrivateKey,
    }

    impl ObjectImpl for Service {}

    #[glib::object_subclass]
    impl ObjectSubclass for Service {
        const NAME: &'static str = "Service";
        type Type = super::Service;
    }
}

glib::wrapper! {
    pub struct Service(ObjectSubclass<imp::Service>);
}

impl Service {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn startup(&self) {
        let private_key = self.imp().private_key.clone();
        info!("my public key: {}", private_key.public_key());

        self.imp()
            .network
            .run(private_key, Hash::new(b"aardvark <3"));
    }

    pub fn shutdown(&self) {
        self.imp().network.shutdown();
    }

    pub(crate) fn create_document(&self) -> (Hash, mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        let (document_id, tx, rx) = self
            .imp()
            .network
            .create_document()
            .expect("to create document");
        info!("created new document: {}", document_id.to_hex());
        (document_id, tx, rx)
    }

    pub(crate) fn join_document(
        &self,
        document_id: Hash,
    ) -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        self.imp()
            .network
            .join_document(document_id)
            .expect("to join document")
    }

    pub(crate) fn public_key(&self) -> PublicKey {
        self.imp().private_key.public_key()
    }
}

impl Default for Service {
    fn default() -> Self {
        Service::new()
    }
}
