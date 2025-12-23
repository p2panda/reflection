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
use gtk::{gdk, gio, glib, glib::Properties, glib::clone};
use reflection_doc::{document::DocumentId, identity::PrivateKey, service::Service};
use std::{cell::RefCell, fs};
use thiserror::Error;
use tracing::error;

use crate::config;
use crate::open_dialog::OpenDialog;
use crate::secret;
use crate::system_settings::SystemSettings;
use crate::window::Window;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Identity(#[from] secret::Error),
    #[error(transparent)]
    Service(#[from] reflection_doc::service::StartupError),
    #[error(transparent)]
    Filesystem(#[from] std::io::Error),
}

mod imp {
    use super::*;

    #[derive(Properties, Default)]
    #[properties(wrapper_type = super::ReflectionApplication)]
    pub struct ReflectionApplication {
        #[property(get, nullable)]
        pub service: RefCell<Option<Service>>,
        pub startup_error: RefCell<Option<Error>>,
        pub service_startup_task: RefCell<Option<glib::JoinHandle<()>>>,
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
            obj.set_accels_for_action("window.close", &["<Control>w"]);
        }
    }

    impl ApplicationImpl for ReflectionApplication {
        fn startup(&self) {
            self.parent_startup();

            gtk::Window::set_default_icon_name(config::APP_ID);
        }

        fn shutdown(&self) {
            glib::MainContext::default().block_on(async move {
                // Make sure service startup finished
                if let Some(handle) = self.service_startup_task.take() {
                    handle
                        .await
                        .expect("Service startup to complete on shutdown");
                }
                if let Some(service) = self.obj().service() {
                    service.shutdown().await;
                }
            });
            self.parent_shutdown();
        }

        fn activate(&self) {
            self.parent_activate();

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
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", config::APP_ID)
            .property("flags", gio::ApplicationFlags::empty())
            .build()
    }

    pub fn window_for_document_id(&self, document_id: &DocumentId) -> Option<Window> {
        self.windows()
            .into_iter()
            .filter_map(|window| window.downcast::<Window>().ok())
            .find(|window| {
                window
                    .document()
                    .is_some_and(|document| &document.id() == document_id)
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
            .activate(move |app: &Self, _, _| {
                app.new_window();
            })
            .build();
        let new_document_action = gio::ActionEntry::builder("new-document")
            .activate(move |app: &Self, _, _| app.new_document())
            .build();
        let join_document_action = gio::ActionEntry::builder("join-document")
            .parameter_type(Some(&glib::VariantType::new_array(
                &DocumentId::static_variant_type(),
            )))
            .activate(move |app: &Self, _, parameter| {
                let parameter = parameter.unwrap();

                if parameter.n_children() == 0 {
                    app.open_join_document_dialog(false);
                } else {
                    for i in 0..parameter.n_children() {
                        if let Some(document_id) = parameter.child_value(i).get() {
                            app.join_document(&document_id, false);
                            // FIXME: open all documents with it's own window
                            break;
                        } else {
                            error!("Failed to join document: Invalid document id specified");
                        }
                    }
                }
            })
            .build();

        let join_document_in_new_window_action =
            gio::ActionEntry::builder("join-document-in-new-window")
                .parameter_type(Some(&glib::VariantType::new_array(
                    &DocumentId::static_variant_type(),
                )))
                .activate(move |app: &Self, _, parameter| {
                    let parameter = parameter.unwrap();

                    if parameter.n_children() == 0 {
                        app.open_join_document_dialog(true);
                    } else {
                        for i in 0..parameter.n_children() {
                            if let Some(document_id) = parameter.child_value(i).get() {
                                app.join_document(&document_id, true);
                                // FIXME: open all documents with it's own window
                                break;
                            } else {
                                error!("Failed to join document: Invalid document id specified");
                            }
                        }
                    }
                })
                .build();

        let delete_document_action = gio::ActionEntry::builder("delete-document")
            .parameter_type(Some(&glib::VariantType::new_array(
                &DocumentId::static_variant_type(),
            )))
            .activate(move |app: &Self, _, parameter| {
                let parameter = parameter.unwrap();

                for i in 0..parameter.n_children() {
                    if let Some(document_id) = parameter.child_value(i).get() {
                        app.delete_document(&document_id);
                        break;
                    } else {
                        error!("Failed to delete document: Invalid document id specified");
                    }
                }
            })
            .build();

        let copy_document_id_action = gio::ActionEntry::builder("copy-document-id")
            .parameter_type(Some(&glib::VariantType::new_array(
                &DocumentId::static_variant_type(),
            )))
            .activate(move |app: &Self, _, parameter| {
                let parameter = parameter.unwrap();

                for i in 0..parameter.n_children() {
                    if let Some(document_id) = parameter.child_value(i).get() {
                        app.copy_document_id(&document_id);
                        break;
                    } else {
                        error!("Failed to copy document id: Invalid document id specified");
                    }
                }
            })
            .build();

        let temporary_identity_action = gio::ActionEntry::builder("new-temporary-identity")
            .activate(move |app: &Self, _, _| {
                glib::spawn_future_local(clone!(
                    #[weak]
                    app,
                    async move {
                        app.new_temporary_identity().await;
                    }
                ));
            })
            .build();

        self.add_action_entries([
            quit_action,
            about_action,
            new_window_action,
            new_document_action,
            join_document_action,
            join_document_in_new_window_action,
            delete_document_action,
            copy_document_id_action,
            temporary_identity_action,
        ]);
    }

    async fn create_service(&self) -> Result<Service, Error> {
        let private_key = secret::get_or_create_identity().await?;

        let mut data_path = glib::user_data_dir();
        data_path.push("Reflection");
        data_path.push(private_key.public_key().to_string());
        fs::create_dir_all(&data_path)?;
        let data_dir = gio::File::for_path(data_path);

        let service = Service::new(&private_key, Some(&data_dir));
        service.startup().await?;

        Ok(service)
    }

    fn new_window(&self) -> Window {
        let window = Window::new(self);

        if let Some(error) = self.imp().startup_error.borrow().as_ref() {
            window.display_startup_error(error);
        }

        if let Some(service) = self.service() {
            window.set_service(Some(service));
        } else if self.imp().service_startup_task.borrow().is_none() {
            let handle = glib::spawn_future_local(clone!(
                #[weak(rename_to = obj)]
                self,
                async move {
                    let service = obj.create_service().await;

                    match service {
                        Ok(service) => {
                            for window in obj.windows() {
                                if let Ok(window) = window.downcast::<Window>() {
                                    window.set_service(Some(&service));
                                }
                            }
                            obj.imp().service.replace(Some(service));
                        }
                        Err(error) => {
                            error!("Failed to start service: {error}");
                            for window in obj.windows() {
                                if let Ok(window) = window.downcast::<Window>() {
                                    window.display_startup_error(&error);
                                }
                            }
                            obj.imp().startup_error.replace(Some(error));
                        }
                    }
                }
            ));

            self.imp().service_startup_task.replace(Some(handle));
        }

        window.present();
        window
    }

    fn new_document(&self) {
        self.join_document(&DocumentId::new(), false);
    }

    fn open_join_document_dialog(&self, new_window: bool) {
        let window = if new_window {
            self.new_window()
        } else if let Some(active) = self.active_window().and_downcast::<Window>() {
            active
        } else {
            self.new_window()
        };

        let dialog = OpenDialog::new();
        adw::prelude::AdwDialogExt::present(&dialog, Some(&window));
    }

    fn join_document(&self, document_id: &DocumentId, new_window: bool) {
        if let Some(window) = self.window_for_document_id(document_id) {
            window.present();
        } else {
            let Some(service) = self.service() else {
                return;
            };

            let window = if new_window {
                self.new_window()
            } else if let Some(active) = self.active_window().and_downcast::<Window>() {
                active
            } else {
                self.new_window()
            };
            let document = service.join_document(document_id);
            window.set_document(Some(&document));
            let hold_guard = self.hold();
            glib::spawn_future_local(clone!(
                #[weak]
                document,
                async move {
                    document.subscribe().await;
                    drop(hold_guard);
                }
            ));
        }
    }

    fn delete_document(&self, document_id: &DocumentId) {
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Delete Document?"))
            .body_use_markup(true)
            .body(gettext("This document may be stored on other devices, and will only be deleted from this one."))
            .default_response("confirm")
            .close_response("cancel")
            .build();

        dialog.add_response("cancel", &gettext("Cancel"));
        dialog.add_response("confirm", &gettext("Delete From This Device"));
        dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);

        if let Some(service) = self.service()
            && let Some(document) = service.documents().document(document_id)
        {
            let hold_guard = self.hold();
            glib::spawn_future_local(clone!(
                #[weak(rename_to = this)]
                self,
                #[strong]
                document,
                async move {
                    let window = this.active_window();
                    if dialog.choose_future(window.as_ref()).await == "confirm" {
                        document.delete().await;
                    }
                    drop(hold_guard);
                }
            ));
        }
    }

    fn copy_document_id(&self, document_id: &DocumentId) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        display.clipboard().set_text(&document_id.to_string());
    }

    async fn new_temporary_identity(&self) {
        let private_key = PrivateKey::new();
        let service = Service::new(&private_key, None);

        if let Err(error) = service.startup().await {
            let error = error.into();
            error!("Failed to start service: {error}");
            for window in self.windows() {
                if let Ok(window) = window.downcast::<Window>() {
                    window.display_startup_error(&error);
                }
            }

            self.imp().startup_error.replace(Some(error));
            // Since the error isn't resolved with a temporary identity disable the action
            self.lookup_action("new-temporary-identity")
                .unwrap()
                .downcast::<gio::SimpleAction>()
                .unwrap()
                .set_enabled(false);

            return;
        }

        for window in self.windows() {
            if let Ok(window) = window.downcast::<Window>() {
                window.set_service(Some(&service));
            }
        }

        self.imp().service.replace(Some(service));
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
