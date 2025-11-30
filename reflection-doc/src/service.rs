use gio::prelude::{FileExt, ListModelExtManual, NetworkMonitorExt};
use glib::object::ObjectExt;
use glib::subclass::prelude::*;
use glib::{Properties, clone};
use reflection_node::p2panda_core::Hash;
use std::sync::{Mutex, OnceLock};
use thiserror::Error;
use tracing::error;

use crate::identity::PrivateKey;
use crate::{document::Document, documents::Documents};
use reflection_node::{
    document::DocumentError,
    node,
    node::{Node, NodeError},
};

#[derive(Error, Debug)]
pub enum StartupError {
    #[error(transparent)]
    Node(#[from] NodeError),
    #[error(transparent)]
    Document(#[from] DocumentError),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, glib::Enum, Default)]
#[repr(u32)]
#[enum_type(name = "ReflectionConnectionMode")]
pub enum ConnectionMode {
    None,
    Bluetooth,
    #[default]
    Network,
}

impl From<ConnectionMode> for node::ConnectionMode {
    fn from(value: ConnectionMode) -> Self {
        match value {
            ConnectionMode::None => node::ConnectionMode::None,
            ConnectionMode::Bluetooth => node::ConnectionMode::Bluetooth,
            ConnectionMode::Network => node::ConnectionMode::Network,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Default, Properties)]
    #[properties(wrapper_type = super::Service)]
    pub struct Service {
        pub node: OnceLock<Node>,
        #[property(get, set, construct_only, type = PrivateKey)]
        pub private_key: OnceLock<PrivateKey>,
        #[property(get, set, construct_only, nullable, type = Option<gio::File>)]
        pub data_dir: OnceLock<Option<gio::File>>,
        #[property(get)]
        documents: Documents,
        #[property(get = Self::connection_mode, set = Self::set_connection_mode, builder(ConnectionMode::default()))]
        pub connection_mode: Mutex<ConnectionMode>,
    }

    impl Service {
        fn set_connection_mode(&self, connection_mode: ConnectionMode) {
            *self.connection_mode.lock().unwrap() = connection_mode;
            glib::spawn_future(clone!(
                #[weak(rename_to = this)]
                self,
                async move {
                    this.update_node_connection_mode().await;
                }
            ));
        }

        fn connection_mode(&self) -> ConnectionMode {
            *self.connection_mode.lock().unwrap()
        }

        pub(super) async fn update_node_connection_mode(&self) {
            let Some(node) = self.node.get() else {
                return;
            };
            let network_available = {
                let monitor = gio::NetworkMonitor::default();
                monitor.is_network_available()
            };
            let connection_mode = (*self.connection_mode.lock().unwrap()).into();
            let wants_network = connection_mode == node::ConnectionMode::Network;
            let real_connection_mode = if !network_available && wants_network {
                node::ConnectionMode::None
            } else {
                connection_mode
            };

            node.set_connection_mode(real_connection_mode)
                .await
                .unwrap();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Service {
        fn constructed(&self) {
            self.parent_constructed();

            let monitor = gio::NetworkMonitor::default();
            monitor.connect_network_available_notify(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    glib::spawn_future(clone!(
                        #[weak]
                        this,
                        async move {
                            this.update_node_connection_mode().await;
                        }
                    ));
                }
            ));
        }
    }

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
    pub fn new(private_key: &PrivateKey, data_dir: Option<&gio::File>) -> Self {
        glib::Object::builder()
            .property("private-key", private_key)
            .property("data-dir", data_dir)
            .build()
    }

    pub async fn startup(&self) -> Result<(), StartupError> {
        let private_key = self.private_key().0;
        let network_id = Hash::new(b"reflection");
        let path = self.data_dir().and_then(|data_dir| data_dir.path());
        let node = Node::new(
            private_key,
            network_id,
            path.as_deref(),
            // gio::NetworkManager is slow to initialize the `network-available` property,
            // so it might be incorrect therefore always start with no connection.
            node::ConnectionMode::None,
        )
        .await?;

        self.imp()
            .node
            .set(node)
            .expect("Service to startup only once");

        self.imp().update_node_connection_mode().await;
        self.documents().load(self).await?;

        Ok(())
    }

    pub async fn shutdown(&self) {
        for document in self.documents().iter::<Document>() {
            document.unwrap().unsubscribe().await;
        }

        if let Err(error) = self.node().shutdown().await {
            error!("Failed to shutdown service: {}", error);
        }
    }

    pub(crate) fn node(&self) -> &Node {
        self.imp().node.get().expect("Service to run")
    }
}
