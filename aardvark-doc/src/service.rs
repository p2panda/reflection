use glib::subclass::prelude::*;
use p2panda_core::{Hash, PrivateKey, PublicKey};
use tracing::{error, info};

use aardvark_node::Node;

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
        glib::MainContext::new().block_on(async move {
            if let Err(error) = self
                .imp()
                .node
                .run(private_key, Hash::new(network_id))
                .await
            {
                error!("Running node failed: {error}");
            }
        });
    }

    pub fn shutdown(&self) {
        self.imp().node.shutdown();
    }

    pub(crate) fn node(&self) -> &Node {
        &self.imp().node
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
