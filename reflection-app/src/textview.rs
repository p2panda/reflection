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

use gtk::{gdk, glib, graphene};
use gtk::{prelude::*, subclass::prelude::*};
use sourceview::subclass::prelude::ViewImpl;

use crate::textbuffer::ReflectionTextBuffer;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct TextView {}

    #[glib::object_subclass]
    impl ObjectSubclass for TextView {
        const NAME: &'static str = "ReflectionTextView";
        type Type = super::TextView;
        type ParentType = sourceview::View;
    }

    impl ObjectImpl for TextView {}
    impl WidgetImpl for TextView {}

    impl TextViewImpl for TextView {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            if layer != gtk::TextViewLayer::AboveText {
                return;
            }

            let buffer: ReflectionTextBuffer = self
                .obj()
                .buffer()
                .downcast()
                .expect("ReflectionTextView needs to have a ReflectionTextBuffer");

            for (author, mark) in buffer.remote_cursors().iter() {
                let color =
                    gdk::RGBA::parse(author.hex_color()).expect("Author color to be in hex format");
                let iter = buffer.iter_at_mark(mark);
                let location = self.obj().iter_location(&iter);
                let aspect_ratio = self.obj().settings().gtk_cursor_aspect_ratio() as f32;
                let cursor_width = location.height() as f32 * aspect_ratio + 1f32;

                // FIXME: Handle angled cursors (e.g. for italic)
                // See draw_insertation_cursor() in gtk/gtkrenderlayout.c for angle calculation
                // GTK uses cairo to draw the cursor, we could use `gtk_snapshot_append_stroke()`
                // or rotate the appended color.

                let bounds = graphene::Rect::new(
                    location.x() as f32,
                    location.y() as f32,
                    cursor_width,
                    location.height() as f32,
                );
                snapshot.append_color(&color, &bounds);
            }
        }
    }

    impl ViewImpl for TextView {}
}

glib::wrapper! {
    pub struct TextView(ObjectSubclass<imp::TextView>)
        @extends gtk::Widget, gtk::TextView, sourceview::View;
}

impl TextView {}
