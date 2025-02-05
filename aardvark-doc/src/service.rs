use glib::subclass::prelude::*;
use p2panda_core::{Hash, PrivateKey, PublicKey};
use tokio::sync::mpsc;
use tracing::info;

use aardvark_node::network;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Service {
        pub network: network::Network,
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

    pub(crate) fn document(&self, id: Hash) -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) {
        self.imp().network.get_or_create_document(id)
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
