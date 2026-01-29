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

use gettextrs::gettext;

use gtk::{
    glib,
    glib::{clone, variant::ToVariant},
    prelude::{ActionableExt, ObjectExt},
};

use reflection_doc::document::DocumentId;

use crate::ReflectionApplication;

mod imp {
    use super::*;

    use adw::prelude::{
        AdwDialogExt, ButtonExt, TextBufferExt, TextBufferExtManual, TextViewExt, WidgetExt,
    };
    use adw::subclass::prelude::{
        AdwDialogImpl, CompositeTemplateClass, CompositeTemplateInitializingExt, WidgetClassExt,
        WidgetImpl, WindowImpl,
    };

    use glib::subclass::prelude::*;
    use gtk::TemplateChild;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(file = "src/open_dialog/open_dialog.blp")]
    pub struct OpenDialog {
        #[template_child]
        pub open_document_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub open_document_entry: TemplateChild<gtk::TextView>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OpenDialog {
        const NAME: &'static str = "ReflectionOpenDialog";
        type Type = super::OpenDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for OpenDialog {
        fn constructed(&self) {
            self.parent_constructed();

            self.open_document_button.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    this.obj().close();
                }
            ));

            self.open_document_entry.buffer().connect_changed(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    let buffer = this.open_document_entry.buffer();
                    let input: String = buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), false)
                        .chars()
                        .filter(|c| c.is_ascii_hexdigit())
                        .collect();

                    let document_id = if input.len() == 64 {
                        DocumentId::from_hex(&input).ok()
                    } else {
                        None
                    };
                    this.open_document_button
                        .set_sensitive(document_id.is_some());

                    let existing = if let Some(document_id) = document_id {
                        this.open_document_button
                            .set_action_target_value(Some(&[document_id].to_variant()));

                        let app = ReflectionApplication::default();
                        app.window_for_document_id(&document_id)
                    } else {
                        None
                    };

                    if existing.is_some() {
                        this.open_document_button
                            .set_label(&gettext("Switch to Existing Window"));
                    } else {
                        this.open_document_button
                            .set_label(&gettext("Open Pad"));
                    }
                }
            ));

            self.open_document_entry
                .buffer()
                .connect_insert_text(|buffer, pos, new_text| {
                    let mut prev_char: Option<char> = None;
                    let filterd_text: String = new_text
                        .chars()
                        .filter(|c| {
                            if c.is_ascii_hexdigit() {
                                prev_char = None;
                                true
                            } else if c == &' ' && prev_char != Some(' ') {
                                prev_char = Some(' ');
                                true
                            } else {
                                false
                            }
                        })
                        .collect();

                    let mut before_iter = *pos;
                    let before_char = if before_iter.backward_char() {
                        Some(before_iter.char())
                    } else {
                        None
                    };
                    let after_char = Some(pos.char());

                    let trimmed_text = if before_char == Some(' ') && after_char == Some(' ') {
                        filterd_text.trim()
                    } else if before_char == Some(' ') {
                        filterd_text.trim_start()
                    } else if after_char == Some(' ') {
                        filterd_text.trim_end()
                    } else {
                        &filterd_text
                    };

                    let input_len = buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), false)
                        .chars()
                        .filter(|c| c.is_ascii_hexdigit())
                        .count();

                    if trimmed_text.is_empty() || (input_len >= 64 && trimmed_text != " ") {
                        buffer.stop_signal_emission_by_name("insert-text");
                    } else if new_text != trimmed_text {
                        buffer.stop_signal_emission_by_name("insert-text");
                        buffer.insert(pos, trimmed_text);
                    }
                });
        }
    }

    impl WidgetImpl for OpenDialog {}
    impl WindowImpl for OpenDialog {}
    impl AdwDialogImpl for OpenDialog {}
}

glib::wrapper! {
    pub struct OpenDialog(ObjectSubclass<imp::OpenDialog>)
        @extends gtk::Widget, adw::Dialog, adw::Window, gtk::Window,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager,
            gtk::Root, gtk::Native;
}

impl OpenDialog {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
