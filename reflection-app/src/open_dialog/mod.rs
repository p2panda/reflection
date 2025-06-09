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

use std::str::FromStr;

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{glib, glib::clone, glib::closure_local};

use reflection_doc::document::DocumentId;

use crate::ReflectionApplication;

mod imp {
    use super::*;
    use adw::prelude::AdwDialogExt;
    use glib::subclass::Signal;
    use std::sync::LazyLock;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/p2panda/reflection/open_dialog/open_dialog.ui")]
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
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    // The user has clicked the open document button in the dialog.
                    Signal::builder("open")
                        .param_types([DocumentId::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            self.open_document_button.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    let open_document_buffer = this.open_document_entry.buffer();
                    let document_id = DocumentId::from_str(
                        &open_document_buffer
                            .text(
                                &open_document_buffer.start_iter(),
                                &open_document_buffer.end_iter(),
                                false,
                            )
                            .chars()
                            .filter(|c| c.is_digit(16))
                            .collect::<String>(),
                    )
                    .expect("valid document id");

                    this.obj().emit_by_name::<()>("open", &[&document_id]);
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
                        .filter(|c| c.is_digit(16))
                        .collect();

                    let document_id = if input.len() == 64 {
                        DocumentId::from_str(&input).ok()
                    } else {
                        None
                    };
                    this.open_document_button
                        .set_sensitive(document_id.is_some());

                    let existing = if let Some(document_id) = document_id {
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
                            .set_label(&gettext("Open Document"));
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
                            if c.is_digit(16) {
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

                    let mut before_iter = pos.clone();
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
                        .filter(|c| c.is_digit(16))
                        .count();

                    if trimmed_text.len() == 0 || (input_len >= 64 && trimmed_text != " ") {
                        buffer.stop_signal_emission_by_name("insert-text");
                    } else if new_text != trimmed_text {
                        buffer.stop_signal_emission_by_name("insert-text");
                        buffer.insert(pos, &trimmed_text);
                    }
                });
        }
    }

    impl WidgetImpl for OpenDialog {}
    impl DialogImpl for OpenDialog {}
    impl WindowImpl for OpenDialog {}
    impl AdwDialogImpl for OpenDialog {}
}

glib::wrapper! {
    pub struct OpenDialog(ObjectSubclass<imp::OpenDialog>)
        @extends gtk::Widget, adw::Dialog, adw::Window;
}

impl OpenDialog {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Connect to the signal emitted when a user clicks a document in the document list.
    pub fn connect_open<F: Fn(&Self, &DocumentId) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        self.connect_closure(
            "open",
            true,
            closure_local!(move |obj: Self, document_id: DocumentId| {
                f(&obj, &document_id);
            }),
        )
    }
}
