/* application.rs
 *
 * Copyright 2024 The Reflection Developers
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

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib, glib::Properties};
use reflection_doc::{document::DocumentId, service::Service};
use std::{cell::OnceCell, fs};
use tracing::error;

use crate::config;
use crate::secret;
use crate::system_settings::SystemSettings;
use crate::window::Window;

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::ReflectionApplication)]
    pub struct ReflectionApplication {
        #[property(get)]
        pub service: OnceCell<Service>,
        #[property(get)]
        pub system_settings: SystemSettings,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReflectionApplication {
        const NAME: &'static str = "ReflectionApplication";
        type Type = super::ReflectionApplication;
        type ParentType = adw::Application;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ReflectionApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_gactions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
            obj.set_accels_for_action("app.new-window", &["<control>n"]);
        }
    }

    impl ApplicationImpl for ReflectionApplication {
        fn startup(&self) {
            glib::MainContext::new().block_on(async move {
                let private_key = secret::get_or_create_identity()
                    .await
                    .expect("Unable to get or create identity");

                let mut data_path = glib::user_data_dir();
                data_path.push("Reflection");
                data_path.push(private_key.public_key().to_string());
                if let Err(error) = fs::create_dir_all(&data_path) {
                    error!("Failed to create data directory: {error}");
                }
                let data_dir = gio::File::for_path(data_path);

                let service = Service::new(&private_key, &data_dir);
                if let Err(error) = service.startup() {
                    error!("Service failed to start: {error}");
                }

                self.service.set(service).unwrap();
            });

            self.parent_startup();
        }

        fn shutdown(&self) {
            self.obj().service().shutdown();
            self.parent_shutdown();
        }

        fn activate(&self) {
            self.obj().new_window();
        }
    }

    impl GtkApplicationImpl for ReflectionApplication {}
    impl AdwApplicationImpl for ReflectionApplication {}
}

glib::wrapper! {
    pub struct ReflectionApplication(ObjectSubclass<imp::ReflectionApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl ReflectionApplication {
    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .build()
    }

    pub fn window_for_document_id(&self, document_id: &DocumentId) -> Option<Window> {
        self.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<Window>().ok())
            .find(|window| {
                window
                    .document()
                    .map_or(false, |document| &document.id() == document_id)
            })
    }

    fn setup_gactions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        let new_window_action = gio::ActionEntry::builder("new-window")
            .activate(move |app: &Self, _, _| app.new_window())
            .build();
        self.add_action_entries([quit_action, about_action, new_window_action]);
    }

    fn new_window(&self) {
        let window = Window::new(self);
        window.set_service(Some(&self.service()));
        window.present();
    }

    fn show_about(&self) {
        let window = self.active_window().unwrap();
        let about = adw::AboutDialog::builder()
            .application_name("Reflection")
            .application_icon(config::APP_ID)
            .license_type(gtk::License::Gpl30)
            .website("https://github.com/p2panda/reflection")
            .issue_url("https://github.com/p2panda/reflection/issues")
            .support_url("https://matrix.to/#/#reflection:gnome.org")
            .version(config::VERSION)
            .copyright(gettext("© 2024-2025 The Reflection Team"))
            .developer_name("The Reflection Developers")
            .developers(vec![
                "ada-magicat",
                "Alyssa Ross",
                "adz",
                "Dominic Letz",
                "Jonas Dreßler",
                "Julian Sparber",
                "Sebastian Wick",
                "Silvio Tomatis",
                "Sam Andreae",
                "Tobias Bernard",
                "glyph",
            ])
            .designers(vec!["Tobias Bernard"])
            .translator_credits(gettext("translator-credits"))
            .build();

        about.present(Some(&window));
    }
}

impl Default for ReflectionApplication {
    fn default() -> Self {
        gio::Application::default()
            .and_downcast::<ReflectionApplication>()
            .unwrap()
    }
}
