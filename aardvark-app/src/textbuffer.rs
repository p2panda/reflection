/* textbuffer.rs
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

use glib::subclass::Signal;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use sourceview::prelude::BufferExt;
use sourceview::subclass::prelude::*;
use sourceview::*;
use std::cell::Cell;
use std::sync::OnceLock;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct AardvarkTextBuffer {
        pub inhibit_emit_text_change: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkTextBuffer {
        const NAME: &'static str = "AardvarkTextBuffer";
        type Type = super::AardvarkTextBuffer;
        type ParentType = sourceview::Buffer;
    }

    impl ObjectImpl for AardvarkTextBuffer {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![Signal::builder("text-change")
                    .param_types([i32::static_type(), i32::static_type(), str::static_type()])
                    .build()]
            })
        }

        fn constructed(&self) {
            let manager = adw::StyleManager::default();
            let buffer = self.obj();

            let language_manager = sourceview::LanguageManager::new();
            let markdown = language_manager.language("markdown");

            buffer.set_language(markdown.as_ref());
            // FIXME: When using subclassing highlight matching brackets causes a crash
            // See: https://gitlab.gnome.org/World/Rust/sourceview5-rs/-/issues/11
            buffer.set_highlight_matching_brackets(false);
            buffer.set_style_scheme(style_scheme().as_ref());

            manager.connect_dark_notify(glib::clone!(
                #[weak]
                buffer,
                move |_| {
                    buffer.set_style_scheme(style_scheme().as_ref());
                }
            ));
        }
    }

    impl TextBufferImpl for AardvarkTextBuffer {
        fn insert_text(&self, iter: &mut gtk::TextIter, new_text: &str) {
            let offset = iter.offset();
            println!("inserting new text {} at pos {}", new_text, offset);
            if !self.inhibit_emit_text_change.get() {
                self.obj()
                    .emit_by_name::<()>("text-change", &[&offset, &0, &new_text]);
            }
            self.parent_insert_text(iter, new_text);
        }

        fn delete_range(&self, start: &mut gtk::TextIter, end: &mut gtk::TextIter) {
            let offset_start = start.offset();
            let offset_end = end.offset();
            println!(
                "deleting range at start {} end {}",
                offset_start, offset_end
            );
            if !self.inhibit_emit_text_change.get() {
                self.obj().emit_by_name::<()>(
                    "text-change",
                    &[&offset_start, &(offset_end - offset_start), &""],
                );
            }
            self.parent_delete_range(start, end);
        }
    }

    impl BufferImpl for AardvarkTextBuffer {}
}

glib::wrapper! {
    pub struct AardvarkTextBuffer(ObjectSubclass<imp::AardvarkTextBuffer>)
        @extends gtk::TextBuffer, sourceview::Buffer;
}

impl AardvarkTextBuffer {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    fn set_inhibit_emit_text_change(&self, inhibit_emit_text_change: bool) {
        self.imp()
            .inhibit_emit_text_change
            .set(inhibit_emit_text_change);
    }

    pub fn splice(&self, pos: i32, del: i32, text: &str) {
        if del != 0 {
            let mut begin = self.iter_at_offset(pos);
            let mut end = self.iter_at_offset(pos + del);
            self.set_inhibit_emit_text_change(true);
            self.delete(&mut begin, &mut end);
            self.set_inhibit_emit_text_change(false);
            return;
        }

        let mut pos_iter = self.iter_at_offset(pos);
        self.set_inhibit_emit_text_change(true);
        self.insert(&mut pos_iter, text);
        self.set_inhibit_emit_text_change(false);
    }

    pub fn full_text(&self) -> String {
        self.text(&self.start_iter(), &self.end_iter(), true).into()
    }
}

fn style_scheme() -> Option<sourceview::StyleScheme> {
    let manager = adw::StyleManager::default();
    let scheme_name = if manager.is_dark() {
        "Adwaita-dark"
    } else {
        "Adwaita"
    };

    sourceview::StyleSchemeManager::default().scheme(scheme_name)
}
