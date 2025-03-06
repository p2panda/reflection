use std::cell::{Cell, OnceCell};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use aardvark_node::document::{DocumentId as DocumentIdNode, SubscribableDocument};
use anyhow::Result;
use glib::prelude::*;
use glib::subclass::{Signal, prelude::*};
use glib::{Properties, clone};
use loro::{ExportMode, LoroDoc, event::Diff};
use p2panda_core::{HashError, PublicKey};
use tracing::error;

use crate::authors::Authors;
use crate::service::Service;

#[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "AardvarkDocumentId", nullable)]
pub struct DocumentId(DocumentIdNode);

impl FromStr for DocumentId {
    type Err = HashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(DocumentId(DocumentIdNode::from_str(value)?))
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

mod imp {
    /// Identifier of container where we handle the text CRDT in a Loro document.
    ///
    /// Loro documents can contain multiple different CRDT types in one document.
    const TEXT_CONTAINER_ID: &str = "document";

    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Document)]
    pub struct Document {
        #[property(name = "text", get = Self::text, type = String)]
        crdt_doc: OnceCell<LoroDoc>,
        #[property(get, construct_only, set = Self::set_id)]
        id: OnceCell<DocumentId>,
        #[property(get, set)]
        ready: Cell<bool>,
        #[property(get, construct_only)]
        service: OnceCell<Service>,
        #[property(get)]
        authors: Authors,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Document {
        const NAME: &'static str = "Document";
        type Type = super::Document;
    }

    impl Document {
        pub fn text(&self) -> String {
            self.crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .get_text(TEXT_CONTAINER_ID)
                .to_string()
        }

        fn set_id(&self, id: Option<DocumentId>) {
            if let Some(id) = id {
                self.id.set(id).expect("Document id can only be set once");
            }
        }

        pub fn splice_text(&self, index: i32, delete_len: i32, chunk: &str) -> Result<()> {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(TEXT_CONTAINER_ID);

            if delete_len == 0 {
                text.insert(index as usize, chunk)
                    .expect("update document after text insertion");
            } else {
                text.delete(index as usize, delete_len as usize)
                    .expect("update document after text removal");
            }

            doc.commit();

            Ok(())
        }

        /// Apply changes to the CRDT from a message received from another peer
        pub fn on_remote_message(&self, bytes: &[u8]) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");

            if let Err(err) = doc.import_with(bytes, "delta") {
                eprintln!("received invalid message: {}", err);
            }
        }

        fn emit_text_inserted(&self, pos: i32, text: String) {
            // Emit the signal on the main thread
            let obj = self.obj();
            glib::source::idle_add_full(
                glib::source::Priority::DEFAULT,
                clone!(
                    #[weak]
                    obj,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        obj.emit_by_name::<()>("text-inserted", &[&pos, &text]);
                        glib::ControlFlow::Break
                    }
                ),
            );
        }

        fn emit_range_deleted(&self, start: i32, end: i32) {
            // Emit the signal on the main thread
            let obj = self.obj();
            glib::source::idle_add_full(
                glib::source::Priority::DEFAULT,
                clone!(
                    #[weak]
                    obj,
                    #[upgrade_or]
                    glib::ControlFlow::Break,
                    move || {
                        obj.emit_by_name::<()>("range-deleted", &[&start, &end]);
                        glib::ControlFlow::Break
                    }
                ),
            );
        }

        fn setup_loro_document(&self) {
            let public_key = self.obj().service().public_key();
            let obj = self.obj();
            let doc = LoroDoc::new();
            // The peer id represents the identity of the author applying local changes (that's
            // essentially us), it needs be strictly unique.
            doc.set_peer_id({
                // Take first 8 bytes of public key (32 bytes) to determine a unique "peer id"
                // which is used to keep authors apart inside the text crdt.
                //
                // TODO(adz): This is strictly speaking not collision-resistant but we're limited
                // here by the 8 bytes / 64 bit from the u64 `PeerId` type from Loro. In practice
                // this should not really be a problem, but it would be nice if the Loro API would
                // change some day.
                let mut buf = [0u8; 8];
                buf[..8].copy_from_slice(&public_key.as_bytes()[..8]);
                u64::from_be_bytes(buf)
            })
            .expect("set peer id for new document");

            let text = doc.get_text(TEXT_CONTAINER_ID);
            doc.subscribe(
                &text.id(),
                Arc::new(clone!(
                    #[weak]
                    obj,
                    move |loro_event| {
                        let text_deltas = loro_event.events.into_iter().filter_map(|event| {
                            if event.is_unknown {
                                return None;
                            }

                            // Loro supports all sorts of CRDTs (Lists, Maps, Counters, etc.),
                            // extract only text deltas.
                            if let Diff::Text(loro_deltas) = event.diff {
                                Some(loro_deltas)
                            } else {
                                None
                            }
                        });

                        // Loro's text deltas are represented as QuillJS "Deltas"
                        // See: https://quilljs.com/docs/delta/
                        for commit in text_deltas {
                            let mut index = 0;
                            for delta in commit {
                                match delta {
                                    loro::TextDelta::Retain { retain, .. } => {
                                        index += retain;
                                    }
                                    loro::TextDelta::Insert { insert, .. } => {
                                        let len = insert.len();
                                        obj.imp().emit_text_inserted(index as i32, insert);
                                        index += len;
                                    }
                                    loro::TextDelta::Delete { delete } => {
                                        obj.imp().emit_range_deleted(
                                            index as i32,
                                            (index + delete) as i32,
                                        );
                                    }
                                }
                            }
                        }
                        obj.notify_text();
                    }
                )),
            )
            .detach();

            doc.subscribe_local_update(Box::new(clone!(
                #[weak]
                obj,
                #[upgrade_or]
                false,
                move |delta_bytes| {
                    let delta_bytes = delta_bytes.to_vec();
                    // Move a strong reference to the Document into the spawn,
                    // to ensure changes are always propagated to the network
                    glib::spawn_future(async move {
                        // Broadcast a "text delta" to all peers and persist the snapshot.
                        //
                        // TODO(adz): We should consider persisting the snapshot every x
                        // times or x seconds, not sure yet what logic makes the most
                        // sense.
                        let snapshot_bytes = obj
                            .imp()
                            .crdt_doc
                            .get()
                            .expect("crdt_doc to be set")
                            .export(ExportMode::Snapshot)
                            .expect("encoded crdt snapshot");

                        if let Err(error) = obj
                            .service()
                            .node()
                            .delta_with_snapshot(obj.id().0, delta_bytes, snapshot_bytes)
                            .await
                        {
                            error!(
                                "Failed to send snapshot of document to the network: {}",
                                error
                            );
                        }
                    });

                    true
                }
            )))
            .detach();

            self.crdt_doc.set(doc).unwrap();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Document {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    Signal::builder("text-inserted")
                        .param_types([glib::types::Type::I32, glib::types::Type::STRING])
                        .build(),
                    Signal::builder("range-deleted")
                        .param_types([glib::types::Type::I32, glib::types::Type::I32])
                        .build(),
                ]
            })
        }

        fn constructed(&self) {
            self.parent_constructed();

            if self.id.get().is_none() {
                let document_id = self
                    .obj()
                    .service()
                    .node()
                    .create_document()
                    .expect("Create document");
                self.set_id(Some(DocumentId(document_id)));
            }

            self.setup_loro_document();

            let obj = self.obj();
            glib::spawn_future(clone!(
                #[weak]
                obj,
                async move {
                    let document_id = obj.id().0;
                    let handle = DocumentHandle(obj.downgrade());
                    if let Err(error) = obj.service().node().subscribe(document_id, handle).await {
                        error!("Failed to subscribe to document: {}", error);
                    }
                }
            ));

            // Add ourself to the list of authors
            self.authors
                .add_this_device(self.obj().service().public_key());
        }
    }
}

glib::wrapper! {
    pub struct Document(ObjectSubclass<imp::Document>);
}
impl Document {
    pub fn new(service: &Service, id: Option<&DocumentId>) -> Self {
        glib::Object::builder()
            .property("service", service)
            .property("id", id)
            .build()
    }

    pub fn insert_text(&self, index: i32, chunk: &str) -> Result<()> {
        self.imp().splice_text(index, 0, chunk)
    }

    pub fn delete_range(&self, index: i32, end: i32) -> Result<()> {
        self.imp().splice_text(index, end - index, "")
    }
}

unsafe impl Send for Document {}
unsafe impl Sync for Document {}

struct DocumentHandle(glib::WeakRef<Document>);

impl SubscribableDocument for DocumentHandle {
    fn bytes_received(&self, _author: PublicKey, data: &[u8]) {
        if let Some(document) = self.0.upgrade() {
            document.imp().on_remote_message(data);
        }
    }

    fn authors_joined(&self, authors: Vec<PublicKey>) {
        if let Some(document) = self.0.upgrade() {
            for author in authors.into_iter() {
                document.authors().add_or_update(author, true);
            }
        }
    }

    fn author_set_online(&self, author: PublicKey, is_online: bool) {
        if let Some(document) = self.0.upgrade() {
            document.authors().add_or_update(author, is_online);
        }
    }
}
