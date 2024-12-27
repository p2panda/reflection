/* window.rs
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

use adw::prelude::AdwDialogExt;
use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib};

use crate::AardvarkTextBuffer;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/p2panda/aardvark/window.ui")]
    pub struct AardvarkWindow {
        // Template widgets
        #[template_child]
        pub text_view: TemplateChild<gtk::TextView>,
        #[template_child]
        pub open_document_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub open_document_dialog: TemplateChild<adw::Dialog>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkWindow {
        const NAME: &'static str = "AardvarkWindow";
        type Type = super::AardvarkWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AardvarkWindow {
        fn constructed(&self) {
            self.parent_constructed();

            let buffer = AardvarkTextBuffer::new();
            self.text_view.set_buffer(Some(&buffer));

            let window = self.obj().clone();
            let dialog = self.open_document_dialog.clone();
            self.open_document_button.connect_clicked(move |_| {
                // @TODO: Send `FromApp::Subscribe` message to network containing the share code
                // and wait for the document to be joined.
                dialog.present(Some(&window));
            });
        }
    }

    impl WidgetImpl for AardvarkWindow {}
    impl WindowImpl for AardvarkWindow {}
    impl ApplicationWindowImpl for AardvarkWindow {}
    impl AdwApplicationWindowImpl for AardvarkWindow {}
}

glib::wrapper! {
    pub struct AardvarkWindow(ObjectSubclass<imp::AardvarkWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl AardvarkWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    pub fn get_text_buffer(&self) -> AardvarkTextBuffer {
        let window = self.imp();
        window.text_view.buffer().downcast().unwrap()
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        self.imp().toast_overlay.add_toast(toast);
    }
}
