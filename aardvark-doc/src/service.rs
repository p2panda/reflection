use glib::Properties;
use glib::object::ObjectExt;
use glib::subclass::prelude::*;
use p2panda_core::Hash;
use std::sync::OnceLock;
use tracing::error;

use crate::identity::PrivateKey;
use aardvark_node::Node;

mod imp {
    use super::*;

    #[derive(Default, Properties)]
    #[properties(wrapper_type = super::Service)]
    pub struct Service {
        pub node: Node,
        #[property(get, set, construct_only, type = PrivateKey)]
        pub private_key: OnceLock<PrivateKey>,
    }

    #[glib::derived_properties]
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
    pub fn new(private_key: &PrivateKey) -> Self {
        glib::Object::builder()
            .property("private-key", private_key)
            .build()
    }

    pub fn startup(&self) {
        let network_id = b"aardvark <3";

        glib::MainContext::new().block_on(async move {
            if let Err(error) = self
                .imp()
                .node
                .run(self.private_key().0.clone(), Hash::new(network_id))
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
}
