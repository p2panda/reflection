use glib::subclass::prelude::*;
use p2panda_core::{Hash, PrivateKey, PublicKey};
use tracing::info;

use aardvark_node::{Node, NodeReceiver, NodeSender};

use crate::document::DocumentId;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Service {
        pub node: Node,
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
        let network_id = b"aardvark <3";
        info!("my public key: {}", private_key.public_key());

        self.imp().node.run(private_key, Hash::new(network_id));
    }

    pub fn shutdown(&self) {
        self.imp().node.shutdown();
    }

    pub(crate) fn create_document(&self) -> (DocumentId, NodeSender, NodeReceiver) {
        let (document_id, tx, rx) = self
            .imp()
            .node
            .create_document()
            .expect("to create document");
        info!("created new document: {}", document_id);
        (DocumentId(document_id), tx, rx)
    }

    pub(crate) fn join_document(&self, document_id: &DocumentId) -> (NodeSender, NodeReceiver) {
        self.imp()
            .node
            .join_document(document_id.0)
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
