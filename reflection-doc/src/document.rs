use std::fmt;
use std::str::FromStr;

use anyhow::Result;
use gio::prelude::ApplicationExtManual;
use glib::prelude::*;
use glib::subclass::{Signal, prelude::*};
use glib::{Properties, clone};
use loro::{ExportMode, LoroDoc, LoroText, event::Diff};
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use reflection_node::document::{DocumentId as DocumentIdNode, SubscribableDocument};
use reflection_node::p2panda_core;
use tracing::error;

use crate::author::Author;
use crate::authors::Authors;
use crate::identity::PublicKey;
use crate::service::Service;

#[derive(Clone, Debug, PartialEq, Eq, glib::Boxed)]
#[boxed_type(name = "ReflectionDocumentId", nullable)]
pub struct DocumentId(pub(crate) DocumentIdNode);

impl FromStr for DocumentId {
    type Err = p2panda_core::HashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(DocumentId(DocumentIdNode::from_str(value)?))
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum EphemerialData {
    Cursor {
        cursor: Option<loro::cursor::Cursor>,
        timestamp: std::time::SystemTime,
    },
}

mod imp {
    use super::*;
    use std::cell::{Cell, OnceCell};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    /// Identifier of container where we handle the text CRDT in a Loro document.
    ///
    /// Loro documents can contain multiple different CRDT types in one document.
    const TEXT_CONTAINER_ID: &str = "document";
    const DOCUMENT_NAME_LENGTH: usize = 32;
    const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Document)]
    pub struct Document {
        #[property(get, construct_only)]
        name: Mutex<Option<String>>,
        #[property(get, construct_only, set)]
        last_accessed: Mutex<Option<glib::DateTime>>,
        #[property(name = "text", get = Self::text, type = String)]
        pub(super) crdt_doc: OnceCell<LoroDoc>,
        #[property(get, construct_only, set = Self::set_id)]
        id: OnceCell<DocumentId>,
        #[property(get, set = Self::set_subscribed)]
        subscribed: Cell<bool>,
        #[property(get, construct_only)]
        service: OnceCell<Service>,
        #[property(get, set = Self::set_authors, construct_only)]
        authors: OnceCell<Authors>,
        snapshot_task: Mutex<Option<glib::SourceId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Document {
        const NAME: &'static str = "Document";
        type Type = super::Document;
    }

    fn extract_name(crdt_text: LoroText) -> Option<String> {
        if crdt_text.is_empty() {
            return None;
        }

        let mut name = String::with_capacity(DOCUMENT_NAME_LENGTH);
        crdt_text.iter(|slice| {
            for char in slice.chars() {
                if char == '\n' {
                    // Only use the first line as name for the document
                    return false;
                } else if char.is_whitespace() || char.is_alphanumeric() {
                    name.push(char);
                }
            }

            name.len() < DOCUMENT_NAME_LENGTH
        });

        if name.trim().len() > 0 {
            Some(name)
        } else {
            None
        }
    }

    impl Document {
        fn set_authors(&self, authors: Option<Authors>) {
            if let Some(authors) = authors {
                self.authors.set(authors).unwrap();
            }
        }

        pub fn text(&self) -> String {
            self.crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .get_text(TEXT_CONTAINER_ID)
                .to_string()
        }

        fn update_name(&self) {
            let crdt_text = self
                .crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .get_text(TEXT_CONTAINER_ID);

            let name = extract_name(crdt_text);

            if name == self.obj().name() {
                return;
            }

            *self.name.lock().unwrap() = name.clone();
            self.obj().notify_name();

            let obj = self.obj();
            glib::spawn_future(clone!(
                #[weak]
                obj,
                async move {
                    let document_id = obj.id().0;
                    if let Err(error) = obj
                        .service()
                        .node()
                        .set_name_for_document(&document_id, name)
                        .await
                    {
                        error!(
                            "Failed to update name for document {}: {}",
                            document_id, error
                        );
                    }
                }
            ));
        }

        fn set_id(&self, id: Option<DocumentId>) {
            if let Some(id) = id {
                self.id.set(id).expect("Document id can only be set once");
            }
        }

        pub fn insert_text(&self, index: usize, chunk: &str) -> Result<()> {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(TEXT_CONTAINER_ID);

            text.insert(index, chunk)?;
            doc.commit();

            Ok(())
        }

        pub fn delete_text(&self, index: usize, len: usize) -> Result<()> {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(TEXT_CONTAINER_ID);

            text.delete(index, len)?;
            doc.commit();

            Ok(())
        }

        pub fn set_insert_cursor(&self, position: usize) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(TEXT_CONTAINER_ID);

            let cursor = EphemerialData::Cursor {
                cursor: text.get_cursor(position, Default::default()),
                timestamp: std::time::SystemTime::now(),
            };

            let cursor_bytes = match encode_cbor(&cursor) {
                Ok(data) => data,
                Err(error) => {
                    error!("Failed to serialize cursor: {}", error);
                    return;
                }
            };

            let obj = self.obj();
            glib::spawn_future(clone!(
                #[weak]
                obj,
                async move {
                    let document_id = obj.id().0;
                    if let Err(error) = obj
                        .service()
                        .node()
                        .ephemeral(document_id, cursor_bytes)
                        .await
                    {
                        error!("Failed to send cursor position: {}", error);
                    }
                }
            ));
        }

        /// Apply changes to the CRDT from a message received from another peer
        pub fn on_remote_message(&self, bytes: Vec<u8>) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");

            if let Err(err) = doc.import_with(&bytes, "delta") {
                error!("received invalid message: {}", err);
            }
        }

        pub fn set_subscribed(&self, subscribed: bool) {
            if self.obj().subscribed() == subscribed {
                return;
            }

            self.subscribed.set(subscribed);

            if subscribed {
                *self.last_accessed.lock().unwrap() = None;

                let obj = self.obj();
                glib::spawn_future(clone!(
                    #[weak]
                    obj,
                    async move {
                        let document_id = obj.id().0;
                        let handle = DocumentHandle(obj.downgrade());
                        if let Err(error) =
                            obj.service().node().subscribe(document_id, handle).await
                        {
                            error!("Failed to subscribe to document: {}", error);
                            obj.imp().set_subscribed(false);
                        }
                    }
                ));
            } else {
                *self.last_accessed.lock().unwrap() = glib::DateTime::now_utc().ok();

                if let Some(task) = self.snapshot_task.lock().unwrap().take() {
                    task.remove();
                }

                let obj = self.obj();
                // Keep the application alive till we completed the unsubscription task
                let guard = gio::Application::default().and_then(|app| Some(app.hold()));
                // Keep a strong reference to the document to ensure the document lives long enough
                glib::spawn_future_local(clone!(
                    #[strong]
                    obj,
                    async move {
                        // Store the latest snapshot before unsubscribing
                        obj.store_snapshot().await;

                        let document_id = obj.id().0;
                        if let Err(error) = obj.service().node().unsubscribe(&document_id).await {
                            error!("Failed to unsubscribe document {}: {}", document_id, error);
                        }
                        drop(guard);
                    }
                ));
            }
            self.obj().notify_last_accessed();
            self.obj().notify_subscribed();
        }

        fn emit_text_inserted(&self, pos: i32, text: String) {
            if pos <= DOCUMENT_NAME_LENGTH as i32 {
                self.update_name();
            }

            self.obj()
                .emit_by_name::<()>("text-inserted", &[&pos, &text]);
        }

        fn emit_range_deleted(&self, start: i32, end: i32) {
            if start <= DOCUMENT_NAME_LENGTH as i32 || end <= DOCUMENT_NAME_LENGTH as i32 {
                self.update_name();
            }

            self.obj()
                .emit_by_name::<()>("range-deleted", &[&start, &end]);
        }

        fn mark_for_snapshot(&self) {
            let mut snapshot_task = self.snapshot_task.lock().unwrap();
            if snapshot_task.is_none() {
                let obj = self.obj();
                let ctx = glib::MainContext::ref_thread_default();
                let handle = ctx.spawn_with_priority(
                    glib::source::Priority::LOW,
                    clone!(
                        #[weak]
                        obj,
                        async move {
                            glib::timeout_future_with_priority(
                                glib::source::Priority::LOW,
                                SNAPSHOT_TIMEOUT,
                            )
                            .await;
                            obj.store_snapshot().await;
                            obj.imp().snapshot_task.lock().unwrap().take();
                        }
                    ),
                );

                *snapshot_task = handle.into_source_id().ok();
            }
        }

        fn setup_loro_document(&self) {
            let public_key = self.obj().service().private_key().public_key();
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
                buf[..8].copy_from_slice(&public_key.0.as_bytes()[..8]);
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
                    obj.imp().mark_for_snapshot();
                    // Move a strong reference to the Document into the spawn,
                    // to ensure changes are always propagated to the network
                    glib::spawn_future(async move {
                        // Broadcast a "text delta" to all peers
                        if let Err(error) =
                            obj.service().node().delta(obj.id().0, delta_bytes).await
                        {
                            error!("Failed to send delta of document to the network: {}", error);
                        }
                    });

                    true
                }
            )))
            .detach();

            self.crdt_doc.set(doc).unwrap();
        }

        pub(super) fn handle_ephemeral_data(&self, author: Author, data: EphemerialData) {
            match data {
                EphemerialData::Cursor { cursor, timestamp } => {
                    // FIXME: check if the timestamp is newer then the previous ephemerial data we got
                    let doc = self.crdt_doc.get().expect("crdt_doc to be set");

                    if !author.is_new_cursor_position(timestamp) {
                        return;
                    }

                    if let Some(cursor) = cursor {
                        let abs_pos = match doc.get_cursor_pos(&cursor) {
                            Ok(pos) => pos.current,
                            Err(error) => {
                                error!(
                                    "Failed to get current cursor position of remote user {}: {error}",
                                    author.name()
                                );
                                return;
                            }
                        };

                        self.obj().emit_by_name::<()>("remote-insert-cursor", &[
                            &author,
                            &(abs_pos.pos as i32),
                        ]);
                    } else {
                        self.obj()
                            .emit_by_name::<()>("remote-insert-cursor", &[&author, &-1i32]);
                    }
                }
            }
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
                    Signal::builder("remote-insert-cursor")
                        .param_types([Author::static_type(), glib::types::Type::I32])
                        .build(),
                ]
            })
        }
        fn dispose(&self) {
            self.set_subscribed(false);
        }

        fn constructed(&self) {
            self.parent_constructed();

            if self.id.get().is_none() {
                let document_id = glib::MainContext::new().block_on(async move {
                    self.obj()
                        .service()
                        .node()
                        .create_document()
                        .await
                        .expect("Create document")
                });
                self.set_id(Some(DocumentId(document_id)));
            }

            self.setup_loro_document();

            self.authors.get_or_init(|| {
                let authors = Authors::new();

                // Add ourself to the list of authors
                authors.add_this_device(self.obj().service().private_key().public_key());
                authors
            });

            self.obj().service().documents().add(self.obj().clone());
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

    pub(crate) fn with_state(
        service: &Service,
        id: Option<&DocumentId>,
        name: Option<&str>,
        last_accessed: Option<&glib::DateTime>,
        authors: &Authors,
    ) -> Self {
        glib::Object::builder()
            .property("service", service)
            .property("id", id)
            .property("authors", authors)
            .property("name", name)
            .property("last-accessed", last_accessed)
            .build()
    }

    pub fn insert_text(&self, pos: i32, text: &str) -> Result<()> {
        self.imp().insert_text(pos as usize, text)
    }

    pub fn delete_range(&self, start_pos: i32, end_pos: i32) -> Result<()> {
        self.imp()
            .delete_text(start_pos as usize, (end_pos - start_pos) as usize)
    }

    pub fn set_insert_cursor(&self, position: i32) {
        self.imp().set_insert_cursor(position as usize);
    }

    /// Persist the snapshot.
    pub(crate) async fn store_snapshot(&self) {
        // FIXME: only store a new snapshot if it changed since the previous snapshot
        let snapshot_bytes = self
            .imp()
            .crdt_doc
            .get()
            .expect("crdt_doc to be set")
            .export(ExportMode::Snapshot)
            .expect("encoded crdt snapshot");
        if let Err(error) = self
            .service()
            .node()
            .snapshot(self.id().0, snapshot_bytes)
            .await
        {
            error!(
                "Failed to send snapshot of document to the network: {}",
                error
            );
        }
    }
}

unsafe impl Send for Document {}
unsafe impl Sync for Document {}

struct DocumentHandle(glib::WeakRef<Document>);

impl SubscribableDocument for DocumentHandle {
    fn bytes_received(&self, author: p2panda_core::PublicKey, data: Vec<u8>) {
        if let Some(document) = self.0.upgrade() {
            let context = glib::MainContext::ref_thread_default();
            context.invoke(move || {
                document.imp().on_remote_message(data);
                document.authors().ensure_author(PublicKey(author));
            });
        }
    }

    fn authors_joined(&self, authors: Vec<p2panda_core::PublicKey>) {
        if let Some(document) = self.0.upgrade() {
            let context = glib::MainContext::ref_thread_default();
            context.invoke(move || {
                for author in authors.into_iter() {
                    document.authors().add_or_update(PublicKey(author), true);
                }
            });
        }
    }

    fn author_set_online(&self, author: p2panda_core::PublicKey, is_online: bool) {
        if let Some(document) = self.0.upgrade() {
            let context = glib::MainContext::ref_thread_default();
            context.invoke(move || {
                document
                    .authors()
                    .add_or_update(PublicKey(author), is_online);
            });
        }
    }

    fn ephemeral_bytes_received(&self, author: p2panda_core::PublicKey, data: Vec<u8>) {
        if let Some(document) = self.0.upgrade() {
            let context = glib::MainContext::ref_thread_default();
            context.invoke(move || {
                if let Ok(data) = decode_cbor(&data[..]) {
                    if let Some(author) = document.authors().author(PublicKey(author)) {
                        document.imp().handle_ephemeral_data(author, data);
                    }
                }
            });
        }
    }
}
