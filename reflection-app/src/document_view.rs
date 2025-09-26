/* window.rs
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

use std::cell::{Cell, OnceCell, RefCell};

use reflection_doc::{
    document::{Document, DocumentId},
    service::Service,
};

use adw::{prelude::*, subclass::prelude::*};
use gtk::{gdk, gio::prelude::ApplicationExtManual, glib, glib::clone};

use crate::{
    ConnectionPopover, OpenPopover, ReflectionApplication, ReflectionTextBuffer, TextView,
    components::{MultilineEntry, ZoomLevelSelector},
};

const BASE_TEXT_FONT_SIZE: f64 = 24.0;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::DocumentView)]
    #[template(resource = "/org/p2panda/reflection/document_view.ui")]
    pub struct DocumentView {
        // Template widgets
        #[template_child]
        pub text_view: TemplateChild<sourceview::View>,
        #[template_child]
        pub open_popover_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub open_popover: TemplateChild<OpenPopover>,
        #[template_child]
        pub share_popover: TemplateChild<gtk::Popover>,
        #[template_child]
        pub share_code_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub copy_code_button: TemplateChild<gtk::Button>,
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
    impl ObjectSubclass for DocumentView {
        const NAME: &'static str = "ReflectionDocumentView";
        type Type = super::DocumentView;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            ZoomLevelSelector::static_type();
            MultilineEntry::static_type();
            OpenPopover::static_type();
            TextView::static_type();
            ConnectionPopover::static_type();

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
    impl ObjectImpl for DocumentView {
        fn constructed(&self) {
            self.parent_constructed();

            let buffer = ReflectionTextBuffer::new();
            self.text_view.set_buffer(Some(&buffer));

            self.font_size.set(BASE_TEXT_FONT_SIZE);
            self.obj().set_font_scale(0.0);
            gtk::style_context_add_provider_for_display(
                &gtk::Widget::display(self.obj().upcast_ref()),
                &self.css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            let scroll_controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
            scroll_controller.set_propagation_phase(gtk::PropagationPhase::Capture);
            let window = self.obj();
            scroll_controller.connect_scroll(clone!(
                #[weak]
                window,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |scroll, _dx, dy| {
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
                }
            ));
            self.obj().add_controller(scroll_controller);

            let zoom_gesture = gtk::GestureZoom::new();
            let prev_delta = Cell::new(0.0);
            zoom_gesture.connect_scale_changed(clone!(
                #[weak]
                window,
                move |_, delta| {
                    if prev_delta.get() == delta {
                        return;
                    }

                    if prev_delta.get() < delta {
                        window.set_font_scale(window.font_scale() + delta);
                    } else {
                        window.set_font_scale(window.font_scale() - delta);
                    }
                    prev_delta.set(delta);
                }
            ));
            self.obj().add_controller(zoom_gesture);

            self.open_popover
                .set_model(self.obj().service().documents());

            self.open_popover.connect_document_activated(clone!(
                #[weak(rename_to = this)]
                self,
                move |_, document| {
                    let app = ReflectionApplication::default();
                    if let Some(window) = app.window_for_document_id(&document.id()) {
                        window.present();
                    } else {
                        this.set_document(document.to_owned());
                        let hold_guard = ReflectionApplication::default().hold();
                        glib::spawn_future_local(clone!(
                            #[weak]
                            document,
                            async move {
                                document.subscribe().await;
                                drop(hold_guard);
                            }
                        ));
                    }
                }
            ));

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

            self.set_document(Document::new(&self.obj().service(), &DocumentId::new()));
        }
    }

    impl DocumentView {
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
                .downcast::<ReflectionTextBuffer>()
                .unwrap()
                .set_document(&document);

            let old_document = self.document.replace(Some(document));

            if let Some(old_document) = old_document {
                // We need to make sure that unsubscribe runs
                // to termination before the app is terminated
                let hold_guard = ReflectionApplication::default().hold();
                glib::spawn_future_local(clone!(
                    #[weak]
                    old_document,
                    async move {
                        old_document.unsubscribe().await;
                        drop(hold_guard);
                    }
                ));
            }

            self.obj().notify("document");
        }

        fn format_document_id(document_id: &DocumentId) -> String {
            document_id
                .to_hex()
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

    impl WidgetImpl for DocumentView {}
    impl BinImpl for DocumentView {}
}

glib::wrapper! {
    pub struct DocumentView(ObjectSubclass<imp::DocumentView>)
        @extends gtk::Widget, gtk::Window, adw::Bin,
         @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl DocumentView {
    pub fn new(service: &Service) -> Self {
        glib::Object::builder().property("service", service).build()
    }
}
