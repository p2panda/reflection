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

use std::marker::PhantomData;

use adw::{prelude::*, subclass::prelude::*};
use gtk::{gio, glib};
use reflection_doc::{document::Document, service::Service};

use crate::{
    application::Error as StartupError, document_view::DocumentView, error_page::ErrorPage,
};

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::Window)]
    #[template(resource = "/org/p2panda/reflection/window.ui")]
    pub struct Window {
        #[template_child]
        pub(super) toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub(super) error_page: TemplateChild<ErrorPage>,
        #[property(get = Self::service, set = Self::set_service, nullable)]
        service: PhantomData<Option<Service>>,
        #[property(get = Self::document, nullable)]
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

            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl Window {
        fn document_view(&self) -> Option<DocumentView> {
            self.main_stack
                .child_by_name("document-view")?
                .downcast()
                .ok()
        }

        fn set_service(&self, service: Option<&Service>) {
            let Some(service) = service else {
                return;
            };

            if let Some(document_view) = self.document_view() {
                if &document_view.service() == service {
                    return;
                }
                self.main_stack.remove(&document_view);
            }

            let document_view = DocumentView::new(service);
            self.main_stack
                .add_named(&document_view, Some("document-view"));
            self.main_stack.set_visible_child(&document_view);
        }

        fn service(&self) -> Option<Service> {
            Some(self.document_view()?.service())
        }

        fn document(&self) -> Option<Document> {
            Some(self.document_view()?.document())
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for Window {
        fn constructed(&self) {
            self.parent_constructed();
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
        @implements gtk::Native, gtk::Root, gio::ActionGroup, gio::ActionMap;
}

impl Window {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    pub fn display_startup_error(&self, error: &StartupError) {
        self.imp().error_page.display_startup_error(error);
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        self.imp().toast_overlay.add_toast(toast);
    }
}
