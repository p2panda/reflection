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

use gtk::{gdk, glib, glib::clone, graphene};
use gtk::{prelude::*, subclass::prelude::*};
use sourceview::subclass::prelude::ViewImpl;

use crate::textbuffer::ReflectionTextBuffer;

mod imp {
    use super::*;

    use std::cell::RefCell;

    #[derive(Debug, Default)]
    pub struct TextView {
        spelling_adapter: RefCell<Option<libspelling::TextBufferAdapter>>,
        buffer_notify_handler:
            RefCell<Option<(glib::WeakRef<ReflectionTextBuffer>, glib::SignalHandlerId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TextView {
        const NAME: &'static str = "ReflectionTextView";
        type Type = super::TextView;
        type ParentType = sourceview::View;

        fn class_init(klass: &mut Self::Class) {
            // HACK: Install the `text.undo` action again so we can have custom undo and
            // we need to ignore the `Buffer::can_undo` property
            klass.install_action("text.undo", None, |view, _, _| {
                let Ok(buffer): Result<ReflectionTextBuffer, _> = view.buffer().downcast() else {
                    return;
                };
                buffer.custom_undo();
                view.scroll_mark_onscreen(&buffer.get_insert());
            });

            // HACK: Install the `text.redo` action again so we can have custom redo and
            // we need to ignore the `Buffer::can_redo` property
            klass.install_action("text.redo", None, |view, _, _| {
                let Ok(buffer): Result<ReflectionTextBuffer, _> = view.buffer().downcast() else {
                    return;
                };

                buffer.custom_redo();
                view.scroll_mark_onscreen(&buffer.get_insert());
            });
        }
    }

    impl ObjectImpl for TextView {
        fn constructed(&self) {
            self.parent_constructed();

            let controller = gtk::GestureClick::new();
            controller.set_button(0);
            controller.connect_pressed(clone!(
                #[weak(rename_to = this)]
                self,
                move |click, n_press, x, y| {
                    let Some(ref adapter) = *this.spelling_adapter.borrow() else {
                        return;
                    };
                    let triggers_context_menu = click
                        .last_event(click.current_sequence().as_ref())
                        .is_some_and(|event| event.triggers_context_menu());

                    if n_press != 1 || !triggers_context_menu {
                        adapter.update_corrections();
                        return;
                    }

                    let buffer = this.obj().buffer();
                    if buffer.selection_bounds().is_some() {
                        adapter.update_corrections();
                        return;
                    }

                    // Move the insert mark to the context menu position, since the spellchecker
                    // looks at the insert mark
                    let (buf_x, buf_y) = this.obj().window_to_buffer_coords(
                        gtk::TextWindowType::Widget,
                        x as i32,
                        y as i32,
                    );
                    if let Some(iter) = this.obj().iter_at_location(buf_x, buf_y) {
                        buffer.select_range(&iter, &iter);
                    }

                    adapter.update_corrections();
                }
            ));
            self.obj().add_controller(controller);

            self.obj().connect_notify_local(
                Some("buffer"),
                clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |view, _| {
                        if let Ok(buffer) = view.buffer().downcast::<sourceview::Buffer>() {
                            let checker = libspelling::Checker::default();
                            let adapter = libspelling::TextBufferAdapter::new(&buffer, &checker);

                            let extra_menu = adapter.menu_model();

                            view.set_extra_menu(Some(&extra_menu));
                            view.insert_action_group("spelling", Some(&adapter));

                            adapter.set_enabled(true);
                            this.spelling_adapter.replace(Some(adapter));
                        }

                        // HACK: The enabled state of the actions `text.undo` and `text.redo` are controlled
                        // via `Buffer.can_undo` and `Buffer.can_redo` properties. but we don't have any way to control the properties

                        let Ok(buffer): Result<ReflectionTextBuffer, _> = view.buffer().downcast()
                        else {
                            return;
                        };

                        let handler_id = buffer.connect_notify_local(
                            // HACK: gtk::TextView does update the action enabled state on every notify,
                            // probably a mistake in GTK.
                            None,
                            clone!(
                                #[weak]
                                view,
                                #[weak]
                                buffer,
                                move |_, _| {
                                    view.action_set_enabled("text.undo", buffer.custom_can_undo());
                                    view.action_set_enabled("text.redo", buffer.custom_can_redo());
                                }
                            ),
                        );
                        let old_handler = view
                            .imp()
                            .buffer_notify_handler
                            .replace(Some((buffer.downgrade(), handler_id)));
                        if let Some((buffer_weak, old_handler_id)) = old_handler
                            && let Some(buffer) = buffer_weak.upgrade() {
                                buffer.disconnect(old_handler_id);
                            }

                        view.action_set_enabled("text.undo", buffer.custom_can_undo());
                        view.action_set_enabled("text.redo", buffer.custom_can_redo());
                    }
                ),
            );
        }
    }
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

            for (author, (mark, selection_mark)) in buffer.remote_cursors().iter() {
                let mut color =
                    gdk::RGBA::parse(author.hex_color()).expect("Author color to be in hex format");

                let iter = buffer.iter_at_mark(mark);
                let selection_iter = buffer.iter_at_mark(selection_mark);

                if iter == selection_iter {
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
                } else {
                    color.set_alpha(0.5);

                    let start_iter = iter.min(selection_iter);
                    let end_iter = iter.max(selection_iter);
                    let start_location = self.obj().iter_location(&start_iter);
                    let end_location = self.obj().iter_location(&end_iter);

                    if start_location.y() == end_location.y() {
                        let bounds = graphene::Rect::new(
                            start_location.x() as f32,
                            start_location.y() as f32,
                            (end_location.x() - start_location.x()) as f32,
                            start_location.height() as f32,
                        );

                        snapshot.append_color(&color, &bounds);
                    } else {
                        let visible_rect = self.obj().visible_rect();

                        // First line that may be partially selected
                        let bounds = graphene::Rect::new(
                            start_location.x() as f32,
                            start_location.y() as f32,
                            (visible_rect.width() - self.obj().right_margin() - start_location.x())
                                as f32,
                            start_location.height() as f32,
                        );
                        snapshot.append_color(&color, &bounds);

                        // Last line that might be partially selected
                        let bounds = graphene::Rect::new(
                            self.obj().left_margin() as f32,
                            end_location.y() as f32,
                            end_location.x() as f32,
                            end_location.height() as f32,
                        );
                        snapshot.append_color(&color, &bounds);

                        // Lines between the first and last selected line
                        let bounds = graphene::Rect::new(
                            self.obj().left_margin() as f32,
                            (start_location.y() + start_location.height()) as f32,
                            (visible_rect.width()
                                - self.obj().right_margin()
                                - self.obj().left_margin()) as f32,
                            (end_location.y() - start_location.y() - end_location.height()) as f32,
                        );
                        snapshot.append_color(&color, &bounds);
                    }
                }
            }
        }
    }

    impl ViewImpl for TextView {}
}

glib::wrapper! {
    pub struct TextView(ObjectSubclass<imp::TextView>)
        @extends gtk::Widget, gtk::TextView, sourceview::View,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl TextView {}
