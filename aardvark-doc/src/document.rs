use std::cell::{Cell, OnceCell, RefCell};
use std::str::FromStr;
use std::sync::OnceLock;

use aardvark_node::NodeCommand;
use anyhow::Result;
use glib::prelude::*;
use glib::subclass::prelude::*;
use glib::subclass::Signal;
use glib::{clone, Properties};
use p2panda_core::Hash;

use crate::crdt::{TextCrdt, TextCrdtEvent, TextDelta};
use crate::service::Service;

mod imp {
    use std::rc::Rc;

    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::Document)]
    pub struct Document {
        #[property(name = "text", get = Self::text, type = String)]
        crdt_doc: Rc<RefCell<Option<TextCrdt>>>,
        #[property(get, construct_only, set = Self::set_id)]
        id: OnceCell<String>,
        #[property(get, set)]
        ready: Cell<bool>,
        #[property(get, construct_only)]
        service: OnceCell<Service>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Document {
        const NAME: &'static str = "Document";
        type Type = super::Document;
    }

    impl Document {
        pub fn text(&self) -> String {
            self.crdt_doc
                .borrow()
                .as_ref()
                .expect("crdt_doc to be set")
                .to_string()
        }

        pub fn set_id(&self, id: Option<String>) {
            if let Some(id) = id {
                self.id.set(id).expect("Document id can only be set once");
            }
        }

        pub fn splice_text(&self, index: i32, delete_len: i32, chunk: &str) -> Result<()> {
            let mut doc_borrow = self.crdt_doc.borrow_mut();
            let doc = doc_borrow.as_mut().expect("crdt_doc to be set");
            if delete_len == 0 {
                doc.insert(index as usize, chunk)
                    .expect("update document after text insertion");
            } else {
                doc.remove(index as usize, delete_len as usize)
                    .expect("update document after text removal");
            }

            Ok(())
        }

        fn on_remote_message(&self, bytes: Vec<u8>) {
            let mut doc_borrow = self.crdt_doc.borrow_mut();
            let doc = doc_borrow.as_mut().expect("crdt_doc to be set");
            if let Err(err) = doc.apply_encoded_delta(&bytes) {
                eprintln!("received invalid message: {}", err);
            }
        }

        fn emit_text_inserted(&self, pos: i32, text: &str) {
            self.obj()
                .emit_by_name::<()>("text-inserted", &[&pos, &text]);
        }

        fn emit_range_deleted(&self, start: i32, end: i32) {
            self.obj()
                .emit_by_name::<()>("range-deleted", &[&start, &end]);
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
            let service = self.service.get().unwrap();
            let (network_tx, mut rx) = if let Some(id) = self.id.get() {
                service.join_document(Hash::from_str(id).expect("Invalid document id"))
            } else {
                let (document_id, network_tx, rx) = service.create_document();
                self.set_id(Some(document_id.to_hex()));
                (network_tx, rx)
            };

            let public_key = service.public_key();
            let crdt_doc = TextCrdt::new({
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
            });

            let crdt_doc_rx = crdt_doc.subscribe();

            self.crdt_doc.replace(Some(crdt_doc));

            glib::spawn_future_local(clone!(
                #[weak(rename_to = this)]
                self,
                async move {
                    while let Some(bytes) = rx.recv().await {
                        this.on_remote_message(bytes);
                    }
                }
            ));

            let crdt_doc = self.crdt_doc.clone();

            glib::spawn_future_local(clone!(
                #[weak(rename_to = this)]
                self,
                async move {
                    while let Ok(event) = crdt_doc_rx.recv().await {
                        match event {
                            TextCrdtEvent::LocalEncoded(delta_bytes) => {
                                // Broadcast a "text delta" to all peers and persist the snapshot.
                                //
                                // TODO(adz): We should consider persisting the snapshot every x
                                // times or x seconds, not sure yet what logic makes the most
                                // sense.
                                let snapshot_bytes = crdt_doc
                                    .borrow()
                                    .as_ref()
                                    .expect("crdt_doc to be set")
                                    .snapshot();

                                if network_tx
                                    .send(NodeCommand::DeltaWithSnapshot {
                                        snapshot_bytes,
                                        delta_bytes,
                                    })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            TextCrdtEvent::Local(_text_deltas) => {
                                // TODO(adz): Later we want to apply changes to the text buffer
                                // here.
                            }
                            TextCrdtEvent::Remote(text_deltas) => {
                                for delta in text_deltas {
                                    match delta {
                                        TextDelta::Insert { index, chunk } => {
                                            this.emit_text_inserted(index as i32, &chunk);
                                        }
                                        TextDelta::Remove { index, len } => {
                                            this.emit_range_deleted(index as i32, (index + len) as i32);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            ));
        }
    }
}

glib::wrapper! {
    pub struct Document(ObjectSubclass<imp::Document>);
}
impl Document {
    pub fn new(service: &Service, id: Option<&str>) -> Self {
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
