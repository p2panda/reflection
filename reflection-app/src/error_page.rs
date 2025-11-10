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

use std::ops::Deref;

use adw::subclass::prelude::*;
use gtk::glib;

use crate::application::Error as StartupError;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(file = "src/error_page.blp")]
    pub struct ErrorPage {
        #[template_child]
        pub(super) main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub(super) secret_service_error: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub(super) network_service_error: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub(super) filesystem_error: TemplateChild<adw::StatusPage>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ErrorPage {
        const NAME: &'static str = "ReflectionErrorPage";
        type Type = super::ErrorPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ErrorPage {}

    impl ObjectImpl for ErrorPage {}

    impl WidgetImpl for ErrorPage {}
    impl BinImpl for ErrorPage {}
    impl AccessibleImpl for ErrorPage {}
}

glib::wrapper! {
    pub struct ErrorPage(ObjectSubclass<imp::ErrorPage>)
        @extends gtk::Widget, adw::Bin,
         @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ErrorPage {
    pub fn new() -> Self {
        glib::Object::new()
    }

    pub fn display_startup_error(&self, error: &StartupError) {
        let visible_child = match error {
            StartupError::Identity(_) => &self.imp().secret_service_error,
            StartupError::Service(_) => &self.imp().network_service_error,
            StartupError::Filesystem(_) => &self.imp().filesystem_error,
        };

        self.imp()
            .main_stack
            .set_visible_child(visible_child.deref());
    }
}

