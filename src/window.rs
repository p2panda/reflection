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

use gtk::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use glib::subclass::Signal;
use std::sync::OnceLock;
use adw::prelude::AdwDialogExt;
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

            let obj = self.obj().clone();
            buffer.connect_changed(move |buffer| {
                let s = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                obj.emit_by_name::<()>("text-changed", &[&s.as_str()]);
            });

            let w = self.obj().clone().upcast::<gtk::Widget>();
            let d = self.open_document_dialog.clone();
            self.open_document_button.connect_clicked(move |_| {
                d.present(Some(&w));
            });
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![Signal::builder("text-changed")
                    .param_types([str::static_type()])
                    .build()]
            })
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

    pub fn splice_text_view(&self, pos: i32, del: i32, text: &str) {
        let window = self.imp();
        let buffer: AardvarkTextBuffer = window.text_view.buffer().downcast().unwrap();

        if del != 0 {
            let mut begin = buffer.iter_at_offset(pos);
            let mut end = buffer.iter_at_offset(pos + del);
            buffer.set_inhibit_emit_text_change(true);
            buffer.delete(&mut begin, &mut end);
            buffer.set_inhibit_emit_text_change(false);
            return;
        }

        let mut pos_iter = buffer.iter_at_offset(pos);
        buffer.set_inhibit_emit_text_change(true);
        buffer.insert(&mut pos_iter, text);
        buffer.set_inhibit_emit_text_change(false);
    }

    pub fn get_text_buffer(&self) -> AardvarkTextBuffer {
        let window = self.imp();
        window.text_view.buffer().downcast().unwrap()
    }
}
