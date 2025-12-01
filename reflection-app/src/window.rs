/* Copyright 2025 The Reflection Developers
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

use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib, glib::clone};
use reflection_doc::{document::Document, documents::Documents, service::Service};

use crate::{
    application::Error as StartupError, document_view::DocumentView, error_page::ErrorPage,
    landing_view::LandingView,
};

mod imp {
    use super::*;

    use std::cell::RefCell;
    use std::marker::PhantomData;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::Window)]
    #[template(file = "src/window.blp")]
    pub struct Window {
        #[template_child]
        pub(super) toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub(super) navigation: TemplateChild<adw::NavigationView>,
        #[template_child]
        landing_page: TemplateChild<LandingView>,
        #[template_child]
        document_page: TemplateChild<DocumentView>,
        #[template_child]
        pub(super) error_page: TemplateChild<ErrorPage>,
        #[property(get, set = Self::set_service, nullable)]
        service: RefCell<Option<Service>>,
        #[property(get = Self::document, set = Self::set_document, nullable)]
        document: PhantomData<Option<Document>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Window {
        const NAME: &'static str = "ReflectionWindow";
        type Type = super::Window;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            DocumentView::static_type();
            ErrorPage::static_type();
            LandingView::static_type();

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl Window {
        fn set_service(&self, service: Option<Service>) {
            if let Some(service) = service.as_ref() {
                self.landing_page.set_model(Some(service.documents()));
                self.navigation.pop_to_tag("landing-page");
            } else {
                self.landing_page.set_model(None::<Documents>);
            }
            self.service.replace(service);
        }

        fn document(&self) -> Option<Document> {
            self.document_page.document()
        }

        fn set_document(&self, document: Option<Document>) {
            if document.is_some() {
                self.navigation.push_by_tag("document-page");
            }

            self.document_page.set_document(document);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();

            self.navigation.connect_popped(clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| {
                    this.document_page.set_document(None::<Document>);
                }
            ));
        }
    }

    impl WidgetImpl for Window {}
    impl WindowImpl for Window {}
    impl ApplicationWindowImpl for Window {}
    impl AdwApplicationWindowImpl for Window {}
}

glib::wrapper! {
    pub struct Window(ObjectSubclass<imp::Window>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gtk::Native, gtk::Root, gio::ActionGroup, gio::ActionMap, gtk::Accessible,
            gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl Window {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    pub fn display_startup_error(&self, error: &StartupError) {
        self.imp().error_page.display_startup_error(error);
        self.imp().navigation.push_by_tag("error-page");
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        self.imp().toast_overlay.add_toast(toast);
    }
}
