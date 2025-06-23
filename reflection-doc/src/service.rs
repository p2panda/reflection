use gio::prelude::FileExt;
use glib::Properties;
use glib::object::ObjectExt;
use glib::subclass::prelude::*;
use reflection_node::p2panda_core::Hash;
use std::sync::OnceLock;
use thiserror::Error;
use tracing::error;

use crate::identity::{PrivateKey, PublicKey};
use crate::{
    author::Author,
    authors::Authors,
    document::{Document, DocumentId},
    documents::Documents,
};
use reflection_node::Node;

#[derive(Error, Debug)]
pub enum StartupError {
    #[error(transparent)]
    Node(#[from] anyhow::Error),
}

mod imp {
    use super::*;

    #[derive(Default, Properties)]
    #[properties(wrapper_type = super::Service)]
    pub struct Service {
        pub node: Node,
        #[property(get, set, construct_only, type = PrivateKey)]
        pub private_key: OnceLock<PrivateKey>,
        #[property(get, set, construct_only, type = gio::File)]
        pub data_dir: OnceLock<gio::File>,
        #[property(get)]
        documents: Documents,
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
    pub fn new(private_key: &PrivateKey, data_dir: &gio::File) -> Self {
        glib::Object::builder()
            .property("private-key", private_key)
            .property("data-dir", data_dir)
            .build()
    }

    pub fn startup(&self) -> Result<(), StartupError> {
        glib::MainContext::new().block_on(async move {
            let private_key = self.private_key().0.clone();
            let public_key = private_key.public_key();
            let network_id = Hash::new(b"reflection");
            let path = self.data_dir().path().expect("Valid file path");
            self.imp()
                .node
                .run(private_key.clone(), network_id, Some(path.as_ref()))
                .await?;

            if let Ok(documents) = self.imp().node.documents().await {
                for document in documents {
                    let last_accessed = document.last_accessed.and_then(|last_accessed| {
                        glib::DateTime::from_unix_utc(last_accessed.timestamp()).ok()
                    });

                    let authors: Vec<Author> = document
                        .authors
                        .iter()
                        .map(|author| {
                            if author.public_key == public_key {
                                Author::for_this_device(&PublicKey(author.public_key))
                            } else {
                                let last_seen = author.last_seen.and_then(|last_seen| {
                                    glib::DateTime::from_unix_utc(last_seen.timestamp()).ok()
                                });
                                Author::with_state(
                                    &PublicKey(author.public_key),
                                    last_seen.as_ref(),
                                )
                            }
                        })
                        .collect();

                    let authors = Authors::from_vec(authors);
                    // The document is inserted automatically in the document list
                    let _document = Document::with_state(
                        self,
                        Some(&DocumentId(document.id)),
                        document.name.as_deref(),
                        last_accessed.as_ref(),
                        &authors,
                    );
                }
            }

            Ok(())
        })
    }

    pub fn shutdown(&self) {
        glib::MainContext::new().block_on(async move {
            if let Err(error) = self.imp().node.shutdown().await {
                error!("Failed to shutdown service: {}", error);
            }
        });
    }

    pub(crate) fn node(&self) -> &Node {
        &self.imp().node
    }
}
