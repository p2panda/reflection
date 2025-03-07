/* window.rs
 *
 * Copyright 2024 The Aardvark Developers
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

use std::cell::{Cell, OnceCell, RefCell};

use aardvark_doc::{document::Document, service::Service};
use adw::prelude::AdwDialogExt;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gdk, gio, glib, glib::clone};
use sourceview::*;

use crate::{
    AardvarkApplication, AardvarkTextBuffer,
    components::{MultilineEntry, ZoomLevelSelector},
};

const BASE_TEXT_FONT_SIZE: f64 = 24.0;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::AardvarkWindow)]
    #[template(resource = "/org/p2panda/aardvark/window.ui")]
    pub struct AardvarkWindow {
        // Template widgets
        #[template_child]
        pub text_view: TemplateChild<sourceview::View>,
        #[template_child]
        pub open_dialog_document_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub open_document_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub open_document_dialog: TemplateChild<adw::Dialog>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub share_popover: TemplateChild<gtk::Popover>,
        #[template_child]
        pub share_code_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub copy_code_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub open_document_entry: TemplateChild<gtk::TextView>,
        pub css_provider: gtk::CssProvider,
        pub font_size: Cell<f64>,
        #[property(get, set = Self::set_font_scale, default = 0.0)]
        pub font_scale: Cell<f64>,
        #[property(get, default = 1.0)]
        pub zoom_level: Cell<f64>,
        #[property(get, construct_only)]
        pub service: OnceCell<Service>,
        #[property(get, type = Document)]
        document: RefCell<Option<Document>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AardvarkWindow {
        const NAME: &'static str = "AardvarkWindow";
        type Type = super::AardvarkWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            ZoomLevelSelector::static_type();
            MultilineEntry::static_type();

            klass.bind_template();

            klass.install_action("window.zoom-in", None, |window, _, _| {
                window.set_font_scale(window.font_scale() + 1.0);
            });
            klass.install_action("window.zoom-out", None, |window, _, _| {
                window.set_font_scale(window.font_scale() - 1.0);
            });
            klass.install_action("window.zoom-one", None, |window, _, _| {
                window.set_font_scale(0.0);
            });

            klass.add_binding_action(
                gdk::Key::plus,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-in",
            );
            klass.add_binding_action(
                gdk::Key::KP_Add,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-in",
            );
            klass.add_binding_action(
                gdk::Key::minus,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-out",
            );
            // gnome-text-editor uses this as well: probably to make it
            // nicer for the US keyboard layout
            klass.add_binding_action(
                gdk::Key::equal,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-out",
            );
            klass.add_binding_action(
                gdk::Key::KP_Subtract,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-out",
            );
            klass.add_binding_action(
                gdk::Key::_0,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-one",
            );
            klass.add_binding_action(
                gdk::Key::KP_0,
                gdk::ModifierType::CONTROL_MASK,
                "window.zoom-one",
            );
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AardvarkWindow {
        fn constructed(&self) {
            self.parent_constructed();

            let buffer = AardvarkTextBuffer::new();
            self.text_view.set_buffer(Some(&buffer));

            self.font_size.set(BASE_TEXT_FONT_SIZE);
            self.obj().set_font_scale(0.0);
            gtk::style_context_add_provider_for_display(
                &self.obj().display(),
                &self.css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            let scroll_controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            scroll_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            let window = self.obj().clone();
            scroll_controller.connect_scroll(move |scroll, _dx, dy| {
                if scroll
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    if dy < 0.0 {
                        window.set_font_scale(window.font_scale() + 1.0);
                    } else {
                        window.set_font_scale(window.font_scale() - 1.0);
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
            self.obj().add_controller(scroll_controller);

            let zoom_gesture = gtk::GestureZoom::new();
            let window = self.obj().clone();
            let prev_delta = Cell::new(0.0);
            zoom_gesture.connect_scale_changed(move |_, delta| {
                if prev_delta.get() == delta {
                    return;
                }

                if prev_delta.get() < delta {
                    window.set_font_scale(window.font_scale() + delta);
                } else {
                    window.set_font_scale(window.font_scale() - delta);
                }
                prev_delta.set(delta);
            });
            self.obj().add_controller(zoom_gesture);

            let window = self.obj().clone();
            let dialog = self.open_document_dialog.clone();
            self.open_dialog_document_button.connect_clicked(move |_| {
                dialog.present(Some(&window));
            });

            self.copy_code_button.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |button| {
                    let document_id = Self::format_document_id(&this.obj().document().id());
                    let clipboard = button.display().clipboard();
                    clipboard.set(&document_id);
                    this.share_popover.popdown();
                }
            ));

            self.open_document_button.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    let open_document_buffer = this.open_document_entry.buffer();
                    let document_id: String = open_document_buffer
                        .text(
                            &open_document_buffer.start_iter(),
                            &open_document_buffer.end_iter(),
                            false,
                        )
                        .chars()
                        .filter(|c| c.is_digit(16))
                        .collect();

                    let app = this
                        .obj()
                        .application()
                        .and_then(|app| app.downcast::<AardvarkApplication>().ok())
                        .expect("Application needs to be a AardvarkApplication");

                    if let Some(window) = app.window_for_document_id(&document_id) {
                        window.present();
                    } else {
                        let document =
                            Document::new(this.service.get().unwrap(), Some(&document_id));
                        this.set_document(document);
                    }
                    this.open_document_dialog.close();
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

                    this.open_document_button.set_sensitive(input.len() == 64);

                    let existing = if input.len() == 64 {
                        let app = this
                            .obj()
                            .application()
                            .and_then(|app| app.downcast::<AardvarkApplication>().ok())
                            .expect("Application needs to be a AardvarkApplication");
                        app.window_for_document_id(&input)
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

            let document = Document::new(self.service.get().unwrap(), None);
            self.set_document(document);
        }
    }

    impl AardvarkWindow {
        fn set_font_scale(&self, value: f64) {
            let font_size = self.font_size.get();

            self.font_scale.set(value);

            let size = (font_size + self.obj().font_scale()).max(1.0);
            self.zoom_level.set(size / font_size);
            self.obj().notify_zoom_level();
            self.css_provider
                .load_from_string(&format!(".sourceview {{ font-size: {size}px; }}"));
            self.obj().action_set_enabled("window.zoom-out", size > 1.0);
        }

        fn set_document(&self, document: Document) {
            let document_id = Self::format_document_id(&document.id());
            self.share_code_label.set_text(&document_id);
            self.text_view
                .buffer()
                .downcast::<AardvarkTextBuffer>()
                .unwrap()
                .set_document(&document);
            self.document.replace(Some(document));

            self.obj().notify("document");
        }

        fn format_document_id(document_id: &str) -> String {
            document_id
                .chars()
                .enumerate()
                .flat_map(|(i, c)| {
                    if i != 0 && i % 4 == 0 {
                        Some(' ')
                    } else {
                        None
                    }
                    .into_iter()
                    .chain(std::iter::once(c))
                })
                .collect::<String>()
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
    pub fn new<P: IsA<gtk::Application>>(application: &P, service: &Service) -> Self {
        glib::Object::builder()
            .property("application", application)
            .property("service", service)
            .build()
    }

    pub fn add_toast(&self, toast: adw::Toast) {
        self.imp().toast_overlay.add_toast(toast);
    }
}
