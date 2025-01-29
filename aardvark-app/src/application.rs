/* application.rs
 *
 * Copyright 2024 Tobias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::cell::{OnceCell, RefCell};

use aardvark_doc::{TextCrdt, TextCrdtEvent, TextDelta};
use aardvark_node::network;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use p2panda_core::PrivateKey;
use tokio::sync::{mpsc, oneshot};

use crate::config::VERSION;
use crate::glib::closure_local;
use crate::{AardvarkTextBuffer, AardvarkWindow};

mod imp {
    use super::*;

    pub struct AardvarkApplication {
        pub window: OnceCell<AardvarkWindow>,
        // TODO(adz): The CRDT and backend channels will be moved into `aardvark-doc` in the next
        // refactoring:
        pub document: TextCrdt,
        pub tx: mpsc::Sender<Vec<u8>>,
        pub rx: RefCell<Option<mpsc::Receiver<Vec<u8>>>>,
        #[allow(dead_code)]
        backend_shutdown: oneshot::Sender<()>,
    }

    impl AardvarkApplication {}

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkApplication {
        const NAME: &'static str = "AardvarkApplication";

        type Type = super::AardvarkApplication;
        type ParentType = adw::Application;

        fn new() -> Self {
            // TODO(adz): We probably want to persist the private key somewhere on the file-system
            // or generate a new one on first start.
            let private_key = PrivateKey::new();
            let public_key = private_key.public_key();
            println!("my public key: {}", public_key);

            let document = TextCrdt::new({
                // Take first 8 bytes of public key (32 bytes) to determine a unique "peer id"
                // which is used to keep authors apart inside the text crdt.
                //
                // @TODO(adz): This is strictly speaking not collision-resistant but we're limited
                // here by the 8 bytes / 64 bit from the u64 `PeerId` type from Loro. In practice
                // this should not really be a problem, but it would be nice if the Loro API would
                // change some day.
                let mut buf = [0u8; 8];
                buf[..8].copy_from_slice(&public_key.as_bytes()[..8]);
                u64::from_be_bytes(buf)
            });

            let (backend_shutdown, tx, rx) =
                network::run(private_key).expect("running p2p backend");

            AardvarkApplication {
                document,
                backend_shutdown,
                tx,
                rx: RefCell::new(Some(rx)),
                window: OnceCell::new(),
            }
        }
    }

    impl ObjectImpl for AardvarkApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_gactions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
        }
    }

    impl ApplicationImpl for AardvarkApplication {
        // We connect to the activate callback to create a window when the application has been
        // launched. Additionally, this callback notifies us when the user tries to launch a
        // "second instance" of the application. When they try to do that, we'll just present any
        // existing window.
        fn activate(&self) {
            let application = self.obj();
            let window = application.get_window();

            // Ask the window manager/compositor to present the window
            window.clone().upcast::<gtk::Window>().present();
        }
    }

    impl GtkApplicationImpl for AardvarkApplication {}
    impl AdwApplicationImpl for AardvarkApplication {}
}

glib::wrapper! {
    pub struct AardvarkApplication(ObjectSubclass<imp::AardvarkApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl AardvarkApplication {
    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .build()
    }

    fn setup_gactions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        self.add_action_entries([quit_action, about_action]);
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let about = adw::AboutDialog::builder()
            .application_name("Aardvark")
            .application_icon("org.p2panda.aardvark")
            .developer_name("The Aardvark Developers")
            .version(VERSION)
            .developers(vec!["Tobias"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(&gettext("translator-credits"))
            .copyright("© 2024 Tobias")
            .build();

        about.present(Some(&window));
    }

    pub fn get_window(&self) -> &AardvarkWindow {
        // Get the current window or create one if necessary
        self.imp().window.get_or_init(|| {
            let window = AardvarkWindow::new(self);

            {
                let application = self.clone();
                let mut rx = self
                    .imp()
                    .rx
                    .take()
                    .expect("rx should be given at this point");

                glib::spawn_future_local(async move {
                    while let Some(bytes) = rx.recv().await {
                        application.on_remote_message(bytes);
                    }
                });
            }

            {
                let application = self.clone();

                let document_rx = self.imp().document.subscribe();
                let network_tx = self.imp().tx.clone();

                glib::spawn_future_local(async move {
                    while let Ok(event) = document_rx.recv().await {
                        match event {
                            TextCrdtEvent::LocalEncoded(bytes) => {
                                if network_tx.send(bytes).await.is_err() {
                                    break;
                                }
                            }
                            TextCrdtEvent::Local(_text_delta) => {
                                // @TODO(adz): Later we want to apply changes to the text buffer
                                // here. Something along the lines of:
                                // application.on_deltas_received(vec![text_delta});
                            }
                            TextCrdtEvent::Remote(text_deltas) => {
                                application.on_deltas_received(text_deltas);
                            }
                        }
                    }
                });
            }

            {
                let application = self.clone();

                // @TODO(adz): At this stage the text buffer was already changed. We should instead
                // intercept the event, forward it here to the document, which handles the change,
                // fires an event (see TextCrdtEvent::Local) which then finally manipulates the
                // text buffer.
                window.get_text_buffer().connect_closure(
                    "text-change",
                    false,
                    closure_local!(|_buffer: AardvarkTextBuffer,
                                    position: i32,
                                    del: i32,
                                    text: &str| {
                        application.on_local_text_change(position as usize, del as usize, text);
                    }),
                );
            }

            window
        })
    }

    fn on_remote_message(&self, bytes: Vec<u8>) {
        let document = &self.imp().document;
        if let Err(err) = document.apply_encoded_delta(&bytes) {
            eprintln!("received invalid message: {}", err);
        }
    }

    fn on_local_text_change(&self, position: usize, del: usize, text: &str) {
        if del == 0 {
            self.imp()
                .document
                .insert(position, text)
                .expect("update document after text insertion");
        } else {
            self.imp()
                .document
                .remove(position, del)
                .expect("update document after text removal");
        }
    }

    fn on_deltas_received(&self, text_deltas: Vec<TextDelta>) {
        let buffer = self.imp().window.get().unwrap().get_text_buffer();
        for delta in text_deltas {
            match delta {
                TextDelta::Insert { index, chunk } => {
                    buffer.splice(index as i32, 0, &chunk);
                }
                TextDelta::Remove { index, len } => {
                    buffer.splice(index as i32, len as i32, "");
                }
            }
        }
    }
}
