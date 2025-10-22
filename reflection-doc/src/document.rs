use std::fmt;
use std::hash::Hash;
use std::sync::Arc;

use anyhow::Result;
use glib::prelude::*;
use glib::subclass::{Signal, prelude::*};
use glib::{Properties, clone};
pub use hex::FromHexError;
use loro::{ExportMode, LoroDoc, LoroText, event::Diff};
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use reflection_node::document::{SubscribableDocument, Subscription as DocumentSubscription};
use reflection_node::p2panda_core;
use tracing::error;

use crate::author::Author;
use crate::authors::Authors;
use crate::identity::PublicKey;
use crate::service::Service;

#[derive(Clone, Debug, PartialEq, Eq, Hash, glib::Boxed)]
#[boxed_type(name = "ReflectionDocumentId", nullable)]
pub struct DocumentId([u8; 32]);

impl From<DocumentId> for [u8; 32] {
    fn from(id: DocumentId) -> Self {
        id.0
    }
}

impl From<[u8; 32]> for DocumentId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl DocumentId {
    pub fn new() -> Self {
        let mut arr = [0u8; 32];
        rand::fill(&mut arr[..]);
        DocumentId(arr)
    }

    pub fn from_hex(hex: &str) -> Result<DocumentId, FromHexError> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(hex, &mut bytes as &mut [u8])?;

        Ok(DocumentId(bytes))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
enum EphemerialData {
    Cursor {
        insert_cursor: Option<loro::cursor::Cursor>,
        selection_bound: Option<loro::cursor::Cursor>,
        timestamp: std::time::SystemTime,
    },
}

mod imp {
    use super::*;
    use std::cell::{Cell, OnceCell};
    use std::sync::{Arc, LazyLock, Mutex, OnceLock, RwLock};
    use std::time::Duration;

    /// Identifier of container where we handle the text CRDT in a Loro document.
    ///
    /// Loro documents can contain multiple different CRDT types in one document.
    static TEXT_CONTAINER_ID: LazyLock<loro::ContainerID> =
        LazyLock::new(|| loro::ContainerID::new_root("document", loro::ContainerType::Text));
    const DOCUMENT_NAME_LENGTH: usize = 32;
    const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Document)]
    pub struct Document {
        #[property(get = Self::main_context, set, nullable)]
        main_context: OnceLock<glib::MainContext>,
        #[property(get, construct_only)]
        name: Mutex<Option<String>>,
        #[property(get, construct_only, set)]
        pub(super) last_accessed: Mutex<Option<glib::DateTime>>,
        #[property(name = "text", get = Self::text, type = String)]
        pub(super) crdt_doc: OnceCell<LoroDoc>,
        pub(super) undo_manager: Mutex<Option<loro::UndoManager>>,
        #[property(get)]
        pub(super) can_undo: Cell<bool>,
        #[property(get)]
        pub(super) can_redo: Cell<bool>,
        #[property(get, construct_only)]
        id: OnceCell<DocumentId>,
        #[property(name = "subscribed", get = Self::subscribed, type = bool)]
        pub(super) subscription: RwLock<Option<Arc<DocumentSubscription<DocumentHandle>>>>,
        #[property(get = Self::service, set = Self::set_service, construct_only, type = Service)]
        service: glib::WeakRef<Service>,
        #[property(get)]
        authors: Authors,
        pub(super) tasks: Mutex<Vec<glib::JoinHandle<()>>>,
        pub(super) snapshot_scheduled: Cell<bool>,

        insert_cursor: RwLock<Option<loro::cursor::Cursor>>,
        selection_bound: RwLock<Option<loro::cursor::Cursor>>,
        final_insert_cursor: RwLock<Option<loro::cursor::Cursor>>,
        final_selection_bound: RwLock<Option<loro::cursor::Cursor>>,
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
        fn main_context(&self) -> glib::MainContext {
            self.main_context
                .get()
                .unwrap_or(&glib::MainContext::ref_thread_default())
                .clone()
        }

        fn service(&self) -> Service {
            self.service.upgrade().unwrap()
        }

        fn set_service(&self, service: &Service) {
            self.service.set(Some(service));
        }

        pub fn text(&self) -> String {
            self.crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .get_text(&*TEXT_CONTAINER_ID)
                .to_string()
        }

        fn update_name(&self) {
            let crdt_text = self
                .crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .get_text(&*TEXT_CONTAINER_ID);

            let name = extract_name(crdt_text);

            if name == self.obj().name() {
                return;
            }

            *self.name.lock().unwrap() = name.clone();
            self.obj().notify_name();

            if let Some(subscription) = self.subscription() {
                let handle = self.main_context().spawn(clone!(
                    #[weak]
                    subscription,
                    async move {
                        if let Err(error) = subscription.set_name(name).await {
                            error!("Failed to update name for document: {error}");
                        }
                    }
                ));
                self.tasks.lock().unwrap().push(handle);
            }
        }

        pub fn insert_text(&self, index: usize, chunk: &str) -> Result<()> {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(&*TEXT_CONTAINER_ID);

            text.insert(index, chunk)?;
            doc.commit();

            Ok(())
        }

        pub fn delete_text(&self, index: usize, len: usize) -> Result<()> {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(&*TEXT_CONTAINER_ID);

            text.delete(index, len)?;
            doc.commit();

            Ok(())
        }

        pub fn set_insert_cursor(&self, position: usize, send: bool) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");
            let text = doc.get_text(&*TEXT_CONTAINER_ID);
            let insert_cursor = text.get_cursor(position, Default::default());

            if self.undo_manager.try_lock().is_err() {
                return;
            }

            *self.insert_cursor.write().unwrap() = insert_cursor;

            if send {
                self.brodcast_ephemeral();
            }
        }

        pub fn brodcast_ephemeral(&self) {
            let cursor_data = EphemerialData::Cursor {
                insert_cursor: self.insert_cursor.read().unwrap().clone(),
                selection_bound: self.selection_bound.read().unwrap().clone(),
                timestamp: std::time::SystemTime::now(),
            };

            let cursor_bytes = match encode_cbor(&cursor_data) {
                Ok(data) => data,
                Err(error) => {
                    error!("Failed to serialize cursor: {}", error);
                    return;
                }
            };

            if let Some(subscription) = self.subscription() {
                let handle = self.main_context().spawn(clone!(
                    #[weak]
                    subscription,
                    async move {
                        if let Err(error) = subscription.send_ephemeral(cursor_bytes).await {
                            error!("Failed to send cursor position: {}", error);
                        }
                    }
                ));
                self.tasks.lock().unwrap().push(handle);
            }
        }

        pub fn set_selection_bound(&self, position: Option<usize>) {
            if self.undo_manager.try_lock().is_err() {
                return;
            }
            let cursor = if let Some(position) = position {
                let doc = self.crdt_doc.get().expect("crdt_doc to be set");
                let text = doc.get_text(&*TEXT_CONTAINER_ID);
                text.get_cursor(position, Default::default())
            } else {
                None
            };

            *self.selection_bound.write().unwrap() = cursor;
        }

        pub(super) fn cursors_pos(&self) -> (usize, Option<usize>) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");

            let insert_cursor = self.final_insert_cursor.read().unwrap();
            let insert_cursor_pos = if let Some(insert_cursor) = insert_cursor.as_ref() {
                match doc.get_cursor_pos(insert_cursor) {
                    Ok(loro::cursor::PosQueryResult { current, .. }) => Some(current.pos),
                    Err(error) => {
                        error!("Failed to get current insert cursor position: {error}");
                        None
                    }
                }
            } else {
                None
            };

            let selection_bound = self.final_selection_bound.read().unwrap();
            let selection_bound_pos = if let Some(selection_bound) = selection_bound.as_ref() {
                match doc.get_cursor_pos(selection_bound) {
                    Ok(loro::cursor::PosQueryResult { current, .. }) => Some(current.pos),
                    Err(error) => {
                        error!("Failed to get current selection bound position: {error}");
                        None
                    }
                }
            } else {
                None
            };

            (insert_cursor_pos.unwrap_or_default(), selection_bound_pos)
        }

        /// Apply changes to the CRDT from a message received from another peer
        pub fn on_remote_message(&self, bytes: Vec<u8>) {
            let doc = self.crdt_doc.get().expect("crdt_doc to be set");

            if let Err(err) = doc.import_with(&bytes, "delta") {
                error!("received invalid message: {}", err);
            }
        }

        fn subscribed(&self) -> bool {
            self.subscription().is_some()
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
            if !self.snapshot_scheduled.get() {
                let obj = self.obj();
                let handle = self.main_context().spawn_with_priority(
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
                            obj.imp().snapshot_scheduled.set(false);
                        }
                    ),
                );
                self.tasks.lock().unwrap().push(handle);

                self.snapshot_scheduled.set(true);
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

            doc.subscribe(
                &*TEXT_CONTAINER_ID,
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

                    if let Some(subscription) = obj.imp().subscription() {
                        let handle = obj.imp().main_context().spawn(clone!(
                            #[weak]
                            subscription,
                            async move {
                                // Broadcast a "text delta" to all peers
                                if let Err(error) = subscription.send_delta(delta_bytes).await {
                                    error!(
                                        "Failed to send delta of document to the network: {}",
                                        error
                                    );
                                }
                            }
                        ));
                        obj.imp().tasks.lock().unwrap().push(handle);
                    }

                    true
                }
            )))
            .detach();

            let mut undo_manager = loro::UndoManager::new(&doc);
            // FIXME: Would be nice to also use `loro::UndoManager::group_start()/group_end()`
            undo_manager.set_merge_interval(1000);

            undo_manager.set_on_push(Some(Box::new(clone!(
                #[weak]
                obj,
                #[upgrade_or_default]
                move |stack_type, _, _| {
                    // The `loro::UndoManager` holds internal locks, so we can't update the `Document.can_undo/can_redo` property inline
                    obj.main_context().spawn(clone!(
                        #[weak]
                        obj,
                        async move {
                            let guard = obj.imp().undo_manager.lock().unwrap();
                            let undo_manager = guard.as_ref().expect("UndoManager exists always");

                            match stack_type {
                                loro::UndoOrRedo::Undo => {
                                    let can_undo = undo_manager.can_undo();
                                    drop(guard);

                                    if obj.can_undo() != can_undo {
                                        obj.imp().can_undo.set(can_undo);
                                        obj.notify_can_undo();
                                    }
                                }
                                loro::UndoOrRedo::Redo => {
                                    let can_undo = undo_manager.can_undo();
                                    drop(guard);

                                    if obj.can_undo() != can_undo {
                                        obj.imp().can_undo.set(can_undo);
                                        obj.notify_can_undo();
                                    }
                                }
                            }
                        }
                    ));

                    let mut meta = loro::UndoItemMeta::new();
                    let insert_cursor = obj.imp().insert_cursor.read().unwrap();
                    let selection_bound = obj.imp().selection_bound.read().unwrap();

                    if let Some(insert_cursor) = insert_cursor.as_ref() {
                        meta.add_cursor(insert_cursor);
                        // Only add selection bound if we have an insert cursor
                        if let Some(selection_bound) = selection_bound.as_ref() {
                            meta.add_cursor(selection_bound);
                        }
                    }

                    meta
                }
            ))));

            undo_manager.set_on_pop(Some(Box::new(clone!(
                #[weak]
                obj,
                move |_, _, mut meta| {
                    *obj.imp().final_insert_cursor.write().unwrap() =
                        meta.cursors.pop().map(|cursor| cursor.cursor);

                    *obj.imp().final_selection_bound.write().unwrap() =
                        meta.cursors.pop().map(|cursor| cursor.cursor);
                }
            ))));

            self.can_undo.set(undo_manager.can_undo());
            self.can_redo.set(undo_manager.can_redo());
            *self.undo_manager.lock().unwrap() = Some(undo_manager);

            self.crdt_doc.set(doc).unwrap();
        }

        pub(super) fn handle_ephemeral_data(&self, author: Author, data: EphemerialData) {
            match data {
                EphemerialData::Cursor {
                    insert_cursor,
                    selection_bound,
                    timestamp,
                } => {
                    let doc = self.crdt_doc.get().expect("crdt_doc to be set");

                    if !author.is_new_cursor_position(timestamp) {
                        return;
                    }

                    if let Some(insert_cursor) = insert_cursor {
                        let abs_insert_cursor = match doc.get_cursor_pos(&insert_cursor) {
                            Ok(pos) => pos.current,
                            Err(error) => {
                                error!(
                                    "Failed to get current insert cursor position of remote user {}: {error}",
                                    author.name()
                                );
                                return;
                            }
                        };

                        let abs_selection_bound = if let Some(selection_bound) = selection_bound {
                            match doc.get_cursor_pos(&selection_bound) {
                                Ok(pos) => pos.current,
                                Err(error) => {
                                    error!(
                                        "Failed to get current selection bound position of remote user {}: {error}",
                                        author.name()
                                    );
                                    abs_insert_cursor.clone()
                                }
                            }
                        } else {
                            abs_insert_cursor.clone()
                        };

                        self.obj().emit_by_name::<()>(
                            "remote-insert-cursor",
                            &[
                                &author,
                                &(abs_insert_cursor.pos as i32),
                                &(abs_selection_bound.pos as i32),
                            ],
                        );
                    } else {
                        self.obj()
                            .emit_by_name::<()>("remote-insert-cursor", &[&author, &-1i32, &-1i32]);
                    }
                }
            }
        }

        pub(super) fn subscription(&self) -> Option<Arc<DocumentSubscription<DocumentHandle>>> {
            self.subscription.read().unwrap().clone()
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
                        .param_types([
                            Author::static_type(),
                            glib::types::Type::I32,
                            glib::types::Type::I32,
                        ])
                        .build(),
                ]
            })
        }
        fn dispose(&self) {
            if !self.tasks.lock().unwrap().is_empty() {
                error!(
                    "Document with ID {} was not unsubscribed before dispose.",
                    self.obj().id()
                );
            }
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.setup_loro_document();

            // Add ourself to the list of authors
            self.authors
                .add_this_device(self.obj().service().private_key().public_key());

            self.obj().service().documents().add(self.obj().clone());
        }
    }
}

glib::wrapper! {
    pub struct Document(ObjectSubclass<imp::Document>);
}
impl Document {
    pub fn new(service: &Service, id: &DocumentId) -> Self {
        glib::Object::builder()
            .property("service", service)
            .property("id", id)
            .build()
    }

    pub fn with_main_context(
        service: &Service,
        id: &DocumentId,
        main_context: &glib::MainContext,
    ) -> Self {
        glib::Object::builder()
            .property("service", service)
            .property("id", id)
            .property("main-context", main_context)
            .build()
    }

    pub(crate) fn with_state(
        service: &Service,
        id: Option<&DocumentId>,
        name: Option<&str>,
        last_accessed: Option<&glib::DateTime>,
    ) -> Self {
        glib::Object::builder()
            .property("service", service)
            .property("id", id)
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

    pub fn undo(&self) -> (i32, Option<i32>) {
        let mut guard = self.imp().undo_manager.lock().unwrap();
        let undo_manager = guard.as_mut().expect("UndoManager exists always");
        if let Err(error) = undo_manager.undo() {
            error!("Failed to undo changes: {error}");
        }

        let can_undo = undo_manager.can_undo();
        let can_redo = undo_manager.can_redo();
        drop(guard);

        if self.can_undo() != can_undo {
            self.imp().can_undo.set(can_undo);
            self.notify_can_undo();
        }

        if self.can_redo() != can_redo {
            self.imp().can_redo.set(can_redo);
            self.notify_can_redo();
        }

        let cursors_pos = self.imp().cursors_pos();
        (cursors_pos.0 as i32, cursors_pos.1.map(|pos| pos as i32))
    }

    pub fn redo(&self) -> (i32, Option<i32>) {
        let mut guard = self.imp().undo_manager.lock().unwrap();
        let undo_manager = guard.as_mut().expect("UndoManager exists always");
        if let Err(error) = undo_manager.redo() {
            error!("Failed to redo changes: {error}");
        }

        let can_undo = undo_manager.can_undo();
        let can_redo = undo_manager.can_redo();
        drop(guard);

        if self.can_undo() != can_undo {
            self.imp().can_undo.set(can_undo);
            self.notify_can_undo();
        }

        if self.can_redo() != can_redo {
            self.imp().can_redo.set(can_redo);
            self.notify_can_redo();
        }

        let cursors_pos = self.imp().cursors_pos();
        (cursors_pos.0 as i32, cursors_pos.1.map(|pos| pos as i32))
    }

    pub fn set_insert_cursor(&self, position: i32, send: bool) {
        self.imp().set_insert_cursor(position as usize, send);
    }

    pub fn set_selection_bound(&self, position: Option<i32>) {
        self.imp()
            .set_selection_bound(position.map(|pos| pos as usize));
    }

    pub async fn subscribe(&self) {
        if self.subscribed() {
            return;
        }

        let handle = DocumentHandle(self.downgrade());
        match self.service().node().subscribe(self.id(), handle).await {
            Ok(subscription) => {
                self.imp()
                    .subscription
                    .write()
                    .unwrap()
                    .replace(Arc::new(subscription));
            }
            Err(error) => {
                error!("Failed to subscribe to document: {}", error);
            }
        }

        *self.imp().last_accessed.lock().unwrap() = None;

        self.notify_last_accessed();
        self.notify_subscribed();
    }

    pub async fn unsubscribe(&self) {
        let subscription = self.imp().subscription.write().unwrap().take();

        if let Some(subscription) = subscription {
            let snapshot_bytes = self
                .imp()
                .crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .export(ExportMode::Snapshot)
                .expect("encoded crdt snapshot");

            if let Err(error) = subscription.send_snapshot(snapshot_bytes).await {
                error!(
                    "Failed to send snapshot of document to the network: {}",
                    error
                );
            }

            let tasks = {
                let mut tasks = self.imp().tasks.lock().unwrap();
                std::mem::take(&mut *tasks)
            };

            for task in tasks {
                if let Err(error) = task.await {
                    error!("Failed to complete task while unsubscribing: {error}");
                }
            }

            if let Err(error) = Arc::into_inner(subscription)
                .expect("Expected to have exactly one strong reference to Arc<Subscription>")
                .unsubscribe()
                .await
            {
                error!("Failed to unsubscribe document: {}", error);
            }
        }

        *self.imp().last_accessed.lock().unwrap() = glib::DateTime::now_utc().ok();

        self.notify_last_accessed();
        self.notify_subscribed();
    }

    /// Persist the snapshot.
    pub(crate) async fn store_snapshot(&self) {
        if let Some(subscription) = self.imp().subscription() {
            // FIXME: only store a new snapshot if it changed since the previous snapshot
            let snapshot_bytes = self
                .imp()
                .crdt_doc
                .get()
                .expect("crdt_doc to be set")
                .export(ExportMode::Snapshot)
                .expect("encoded crdt snapshot");
            if let Err(error) = subscription.send_snapshot(snapshot_bytes).await {
                error!(
                    "Failed to send snapshot of document to the network: {}",
                    error
                );
            }
        }
    }
}

unsafe impl Send for Document {}
unsafe impl Sync for Document {}

struct DocumentHandle(glib::WeakRef<Document>);

impl SubscribableDocument for DocumentHandle {
    fn bytes_received(&self, author: p2panda_core::PublicKey, data: Vec<u8>) {
        if let Some(document) = self.0.upgrade() {
            document.main_context().invoke(move || {
                document.imp().on_remote_message(data);
                document.authors().add(PublicKey(author));
            });
        }
    }

    fn author_joined(&self, author: p2panda_core::PublicKey) {
        if let Some(document) = self.0.upgrade() {
            document.main_context().invoke(move || {
                let author = document.authors().add(PublicKey(author));
                author.set_online(true);
                // When a new author joins we need to send ephemeral messages again
                document.imp().brodcast_ephemeral();
            });
        }
    }

    fn author_left(&self, author: p2panda_core::PublicKey) {
        if let Some(document) = self.0.upgrade() {
            document.main_context().invoke(move || {
                let author = document.authors().add(PublicKey(author));
                author.set_online(false);
            });
        }
    }

    fn ephemeral_bytes_received(&self, author: p2panda_core::PublicKey, data: Vec<u8>) {
        if let Some(document) = self.0.upgrade() {
            document.main_context().invoke(move || {
                if let Ok(data) = decode_cbor(&data[..]) {
                    if let Some(author) = document.authors().author(&PublicKey(author)) {
                        document.imp().handle_ephemeral_data(author, data);
                    }
                }
            });
        }
    }
}
