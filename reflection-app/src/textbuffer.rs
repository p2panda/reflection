/* textbuffer.rs
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

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::HashMap,
};

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{glib, glib::clone};
use reflection_doc::{author::Author, document::Document};
use sourceview::prelude::BufferExt;
use sourceview::subclass::prelude::*;
use sourceview::*;
use tracing::{debug, error, info};

mod imp {
    use super::*;

    use crate::ReflectionApplication;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ReflectionTextBuffer)]
    pub struct ReflectionTextBuffer {
        pub inhibit_text_change: Cell<bool>,
        pub document_handlers: OnceCell<glib::SignalGroup>,
        pub changed_handler: RefCell<Option<glib::SignalHandlerId>>,
        #[property(get, set = Self::set_document, nullable)]
        pub document: RefCell<Option<Document>>,
        #[property(name = "custom-can-undo", get = Self::custom_can_undo, type = bool)]
        #[property(name = "custom-can-redo", get = Self::custom_can_redo, type = bool)]
        pub(super) remote_cursors: RefCell<HashMap<Author, (gtk::TextMark, gtk::TextMark)>>,
    }

    impl ReflectionTextBuffer {
        fn custom_can_undo(&self) -> bool {
            self.obj()
                .document()
                .is_some_and(|document| document.can_undo())
        }

        fn custom_can_redo(&self) -> bool {
            self.obj()
                .document()
                .is_some_and(|document| document.can_redo())
        }

        fn set_document(&self, document: Option<&Document>) {
            if let Some(document) = document.as_ref() {
                if let Some(changed_handler) = self.changed_handler.take() {
                    self.obj().disconnect(changed_handler);
                }
                self.obj().set_inhibit_text_change(true);
                self.obj().set_text(&document.text());
                self.obj().set_inhibit_text_change(false);
            }

            if document.is_some_and(|document| !document.subscribed()) {
                let handle = self.obj().connect_changed(move |obj| {
                    if let Some(changed_handler) = obj.imp().changed_handler.take() {
                        obj.disconnect(changed_handler);
                    }
                    // We need to make sure that subscription runs
                    // to termination before the app is terminated
                    let guard = ReflectionApplication::default().hold();
                    glib::spawn_future_local(clone!(
                        #[weak]
                        obj,
                        async move {
                            if let Some(document) = obj.document() {
                                document.subscribe().await;
                                drop(guard)
                            }
                        }
                    ));
                });

                self.changed_handler.replace(Some(handle));
            }

            self.remote_cursors.take();
            self.document_handlers.get().unwrap().set_target(document);
            self.document.replace(document.cloned());
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ReflectionTextBuffer {
        const NAME: &'static str = "ReflectionTextBuffer";
        type Type = super::ReflectionTextBuffer;
        type ParentType = sourceview::Buffer;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ReflectionTextBuffer {
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

            // We could use a signal group to block handlers
            let document_handlers = glib::SignalGroup::with_type(Document::static_type());
            document_handlers.connect_local(
                "text-inserted",
                false,
                clone!(
                    #[weak]
                    buffer,
                    #[upgrade_or]
                    None,
                    move |values| {
                        let pos: i32 = values.get(1).unwrap().get().unwrap();
                        let text: &str = values.get(2).unwrap().get().unwrap();
                        if buffer.inhibit_text_change() {
                            return None;
                        }

                        let mut pos_iter = buffer.iter_at_offset(pos);
                        buffer.set_inhibit_text_change(true);
                        buffer.insert(&mut pos_iter, text);
                        buffer.set_inhibit_text_change(false);

                        None
                    }
                ),
            );

            document_handlers.connect_local(
                "range-deleted",
                false,
                clone!(
                    #[weak]
                    buffer,
                    #[upgrade_or]
                    None,
                    move |values| {
                        let start: i32 = values.get(1).unwrap().get().unwrap();
                        let end: i32 = values.get(2).unwrap().get().unwrap();
                        if buffer.inhibit_text_change() {
                            return None;
                        }

                        let mut start = buffer.iter_at_offset(start);
                        let mut end = buffer.iter_at_offset(end);
                        buffer.set_inhibit_text_change(true);
                        buffer.delete(&mut start, &mut end);
                        buffer.set_inhibit_text_change(false);

                        None
                    }
                ),
            );

            document_handlers.connect_local(
                "remote-insert-cursor",
                false,
                clone!(
                    #[weak]
                    buffer,
                    #[upgrade_or]
                    None,
                    move |values| {
                        let author: Author = values.get(1).unwrap().get().unwrap();
                        let pos: i32 = values.get(2).unwrap().get().unwrap();
                        let selection_bound_pos: i32 = values.get(3).unwrap().get().unwrap();
                        let mut remote_cursors = buffer.imp().remote_cursors.borrow_mut();
                        let author_name = author.name();

                        if pos < 0 {
                            if let Some((mark, selection_bound_mark)) =
                                remote_cursors.remove(&author)
                            {
                                buffer.delete_mark(&mark);
                                buffer.delete_mark(&selection_bound_mark);
                                debug!("Cursor mark for author {author_name} was removed");
                            }
                            return None;
                        }

                        let (mark, selection_bound_mark) =
                            remote_cursors.entry(author).or_insert_with(|| {
                                (
                                    gtk::TextMark::new(None, false),
                                    gtk::TextMark::new(None, false),
                                )
                            });
                        let iter = buffer.iter_at_offset(pos);

                        // New markers are deleted so we need to add them when we create them
                        if mark.is_deleted() {
                            buffer.add_mark(mark, &iter);
                        } else {
                            buffer.move_mark(mark, &iter);
                        }

                        let iter = buffer.iter_at_offset(selection_bound_pos);
                        if selection_bound_mark.is_deleted() {
                            buffer.add_mark(selection_bound_mark, &iter);
                        } else {
                            buffer.move_mark(selection_bound_mark, &iter);
                        }

                        // WORKAROUND: We need to invalidate the display cache,
                        // no idea if there is a better way to do this
                        mark.set_visible(true);
                        mark.set_visible(false);
                        selection_bound_mark.set_visible(true);
                        selection_bound_mark.set_visible(false);

                        debug!("Cursor mark for author {author_name} was added at {pos}");

                        None
                    }
                ),
            );

            document_handlers.connect_notify_local(
                Some("can-undo"),
                clone!(
                    #[weak]
                    buffer,
                    move |_, _| {
                        buffer.notify_custom_can_undo();
                    }
                ),
            );

            document_handlers.connect_notify_local(
                Some("can-redo"),
                clone!(
                    #[weak]
                    buffer,
                    move |_, _| {
                        buffer.notify_custom_can_redo();
                    }
                ),
            );

            self.document_handlers.set(document_handlers).unwrap();

            // Undo/Redo is handled by the CRDT document
            self.obj().set_enable_undo(false);

            self.obj()
                .connect_notify_local(Some("cursor-position"), |obj, _| {
                    // FIXME: the document could calculate the position based on the text inserted/deleted by us
                    if let Some(document) = obj.document() {
                        document.set_insert_cursor(obj.cursor_position(), false);
                        if obj.has_selection() {
                            let selection_bound_iter = obj.iter_at_mark(&obj.selection_bound());
                            document.set_selection_bound(Some(selection_bound_iter.offset()));
                        } else {
                            document.set_selection_bound(None);
                        }
                    }
                });
        }
    }

    impl TextBufferImpl for ReflectionTextBuffer {
        fn insert_text(&self, iter: &mut gtk::TextIter, new_text: &str) {
            if self.obj().inhibit_text_change() {
                self.parent_insert_text(iter, new_text);
                return;
            }
            let Some(document) = self.obj().document() else {
                self.parent_insert_text(iter, new_text);
                return;
            };

            let offset = iter.offset();
            self.obj().set_inhibit_text_change(true);
            let result = document.insert_text(offset, new_text);
            self.obj().set_inhibit_text_change(false);

            // Only insert text into the buffer when the document was successfully updated
            if let Err(error) = result {
                error!("Failed to submit changes to the document: {error}");
            } else {
                info!("inserting new text {} at pos {}", new_text, offset);
                self.parent_insert_text(iter, new_text);
            }
        }

        fn delete_range(&self, start: &mut gtk::TextIter, end: &mut gtk::TextIter) {
            if self.obj().inhibit_text_change() {
                self.parent_delete_range(start, end);
                return;
            }
            let Some(document) = self.obj().document() else {
                self.parent_delete_range(start, end);
                return;
            };

            let offset_start = start.offset();
            let offset_end = end.offset();
            self.obj().set_inhibit_text_change(true);
            let result = document.delete_range(offset_start, offset_end);
            self.obj().set_inhibit_text_change(false);

            // Only delete text from the buffer when the document was successfully updated
            if let Err(error) = result {
                error!("Failed to submit changes to the document: {error}");
            } else {
                info!(
                    "deleting range at start {} end {}",
                    offset_start, offset_end
                );

                self.parent_delete_range(start, end);
            }
        }

        fn mark_set(&self, location: &gtk::TextIter, mark: &gtk::TextMark) {
            if let Some(name) = mark.name() {
                match name.as_str() {
                    "insert" => {
                        if let Some(document) = self.obj().document() {
                            document.set_insert_cursor(location.offset(), true);
                        }
                    }
                    "selection_bound" => {
                        if let Some(document) = self.obj().document() {
                            if self.obj().has_selection() {
                                document.set_selection_bound(Some(location.offset()));
                            } else {
                                document.set_selection_bound(None);
                            }

                            document.set_insert_cursor(self.obj().cursor_position(), true);
                        }
                    }
                    _ => {}
                }
            }

            self.parent_mark_set(location, mark);
        }
    }

    impl BufferImpl for ReflectionTextBuffer {}
}

glib::wrapper! {
    pub struct ReflectionTextBuffer(ObjectSubclass<imp::ReflectionTextBuffer>)
        @extends gtk::TextBuffer, sourceview::Buffer;
}

impl ReflectionTextBuffer {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    fn inhibit_text_change(&self) -> bool {
        self.imp().inhibit_text_change.get()
    }

    fn set_inhibit_text_change(&self, inhibit_text_change: bool) {
        self.imp().inhibit_text_change.set(inhibit_text_change);
    }

    pub fn full_text(&self) -> String {
        self.text(&self.start_iter(), &self.end_iter(), true).into()
    }

    pub fn remote_cursors(&self) -> HashMap<Author, (gtk::TextMark, gtk::TextMark)> {
        self.imp().remote_cursors.borrow().to_owned()
    }

    pub fn custom_undo(&self) {
        if let Some(document) = self.document() {
            let (insert_cursor, selection_bound) = document.undo();

            let insert_cursor_iter = self.iter_at_offset(insert_cursor);
            let selection_bound_iter = if let Some(selection_bound) = selection_bound {
                self.iter_at_offset(selection_bound)
            } else {
                insert_cursor_iter
            };

            self.select_range(&insert_cursor_iter, &selection_bound_iter);
        }
    }

    pub fn custom_redo(&self) {
        if let Some(document) = self.document() {
            let (insert_cursor, selection_bound) = document.redo();

            let insert_cursor_iter = self.iter_at_offset(insert_cursor);
            let selection_bound_iter = if let Some(selection_bound) = selection_bound {
                self.iter_at_offset(selection_bound)
            } else {
                insert_cursor_iter
            };

            self.select_range(&insert_cursor_iter, &selection_bound_iter);
        }
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
