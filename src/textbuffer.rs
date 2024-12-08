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

use gtk::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use glib::subclass::Signal;
use std::sync::OnceLock;
use std::cell::Cell;

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
        type ParentType = gtk::TextBuffer;
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
    }

    impl TextBufferImpl for AardvarkTextBuffer {
        fn insert_text(&self, iter: &mut gtk::TextIter, new_text: &str) {
            let offset = iter.offset();
            println!("inserting new text {} at pos {}", new_text, offset);
            if !self.inhibit_emit_text_change.get() {
                self.obj().emit_by_name::<()>("text-change", &[&offset, &0, &new_text]);
            }
            self.parent_insert_text(iter, new_text);
        }

        fn delete_range(&self, start: &mut gtk::TextIter, end: &mut gtk::TextIter) {
            let offset_start = start.offset();
            let offset_end = end.offset();
            println!("deleting range at start {} end {}", offset_start, offset_end);
            if !self.inhibit_emit_text_change.get() {
                self.obj().emit_by_name::<()>("text-change", &[&offset_start, &(offset_end - offset_start), &""]);
            }
            self.parent_delete_range(start, end);
        }
    }
}

glib::wrapper! {
    pub struct AardvarkTextBuffer(ObjectSubclass<imp::AardvarkTextBuffer>)
        @extends gtk::TextBuffer;
}

impl AardvarkTextBuffer {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn set_inhibit_emit_text_change(&self, inhibit_emit_text_change: bool) {
        self.imp().inhibit_emit_text_change.set(inhibit_emit_text_change);
    }
}
