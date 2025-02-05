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

use aardvark_node::network;
use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use p2panda_core::{Hash, PrivateKey};
use tokio::sync::mpsc;

use crate::config::VERSION;
use crate::AardvarkWindow;

mod imp {
    use super::*;

    pub struct AardvarkApplication {
        pub window: OnceCell<AardvarkWindow>,
        pub tx: mpsc::Sender<Vec<u8>>,
        pub rx: RefCell<Option<mpsc::Receiver<Vec<u8>>>>,
        #[allow(dead_code)]
        network: network::Network,
    }

    impl AardvarkApplication {}

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkApplication {
        const NAME: &'static str = "AardvarkApplication";
        type Type = super::AardvarkApplication;
        type ParentType = adw::Application;

        fn new() -> Self {
            let private_key = PrivateKey::new();
            let public_key = private_key.public_key();
            let network = network::Network::new();
            println!("The public key used: {}", public_key);

            network.run(private_key, Hash::new(b"aardvark <3"));
            let (tx, rx) = network.get_or_create_document(Hash::new(b"some document"));

            AardvarkApplication {
                network,
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
        self.imp().window.get_or_init(|| AardvarkWindow::new(self))
    }
}
