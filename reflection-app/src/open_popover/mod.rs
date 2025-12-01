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
    glib::clone,
    glib::closure_local,
    prelude::{IsA, ObjectExt},
};

use crate::open_dialog::OpenDialog;
use crate::utils::format_datetime;
use reflection_doc::{document::Document, documents::Documents};

mod imp {
    use super::*;

    use adw::prelude::{
        AdwDialogExt, ButtonExt, Cast, EditableExt, ListBoxRowExt, ListModelExt, PopoverExt,
        StaticType, WidgetExt,
    };
    use adw::subclass::prelude::{
        CompositeTemplateClass, CompositeTemplateInitializingExt, PopoverImpl, WidgetClassExt,
        WidgetImpl,
    };
    use glib::subclass::Signal;
    use glib::subclass::prelude::*;
    use gtk::TemplateChild;
    use std::sync::LazyLock;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::OpenPopover)]
    #[template(file = "src/open_popover/open_popover.blp")]
    pub struct OpenPopover {
        #[template_child]
        search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        stack: TemplateChild<gtk::Stack>,
        #[template_child]
        no_results_page: TemplateChild<gtk::Widget>,
        #[template_child]
        document_list_page: TemplateChild<gtk::Widget>,
        #[template_child]
        open_document_button: TemplateChild<gtk::Button>,
        #[property(get = Self::model, set = Self::set_model, type = Option<Documents>)]
        model: gtk::FilterListModel,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OpenPopover {
        const NAME: &'static str = "ReflectionOpenPopover";
        type Type = super::OpenPopover;
        type ParentType = gtk::Popover;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for OpenPopover {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    // The user has activated a document in the document list.
                    Signal::builder("document-activated")
                        .param_types([Document::static_type()])
                        .build(),
                ]
            });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();

            // TODO: We should also match the document id with a more complex filter
            let filter = gtk::StringFilter::builder()
                .expression(gtk::PropertyExpression::new(
                    Document::static_type(),
                    gtk::Expression::NONE,
                    "name",
                ))
                .ignore_case(true)
                .match_mode(gtk::StringFilterMatchMode::Substring)
                .build();
            self.model.set_filter(Some(&filter));

            self.search_entry
                .connect_search_changed(move |search_entry| {
                    filter.set_search(Some(&search_entry.text()));
                });

            self.model.connect_items_changed(clone!(
                #[weak(rename_to = this)]
                self,
                move |model, _, _, _| {
                    if model.n_items() > 0 {
                        this.stack.set_visible_child(&*this.document_list_page);
                    } else {
                        this.stack.set_visible_child(&*this.no_results_page);
                    }
                }
            ));

            self.listbox.connect_row_activated(clone!(
                #[weak(rename_to = this)]
                self,
                move |_, row| {
                    let document: Document = this
                        .model
                        .item(row.index() as u32)
                        .unwrap()
                        .downcast()
                        .unwrap();
                    this.obj()
                        .emit_by_name::<()>("document-activated", &[&document]);
                    this.search_entry.set_text("");
                    this.obj().popdown();
                }
            ));

            self.listbox.bind_model(Some(&self.model), |document| {
                let document = document.downcast_ref::<Document>().unwrap();
                let row = adw::ActionRow::builder()
                    .selectable(false)
                    .activatable(true)
                    .build();

                document
                    .bind_property("name", &row, "title")
                    .sync_create()
                    .transform_to(|_, title: Option<String>| {
                        if let Some(title) = title {
                            Some(title)
                        } else {
                            Some(gettext("Empty document"))
                        }
                    })
                    .build();

                document
                    .bind_property("last-accessed", &row, "subtitle")
                    .sync_create()
                    .transform_to(|binding, last_accessed: Option<glib::DateTime>| {
                        let document: Document = binding.source().unwrap().downcast().unwrap();
                        if let Some(last_accessed) = last_accessed {
                            Some(format_datetime(&gettext("Last accessed"), &last_accessed))
                        } else if document.subscribed() {
                            Some(gettext("Currently open"))
                        } else {
                            Some(gettext("Never accessed"))
                        }
                    })
                    .build();

                row.upcast()
            });

            self.open_document_button.connect_clicked(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    let dialog = OpenDialog::new();
                    let window = this.obj().root();

                    this.obj().popdown();
                    dialog.present(window.as_ref());

                    dialog.connect_open(clone!(
                        #[weak]
                        this,
                        move |_, document_id| {
                            let service = crate::ReflectionApplication::default()
                                .service()
                                .expect("Service to run");
                            let document = service
                                .documents()
                                .document(document_id)
                                .unwrap_or_else(|| service.join_document(document_id));

                            this.obj()
                                .emit_by_name::<()>("document-activated", &[&document]);
                        }
                    ));
                }
            ));
        }
    }

    impl OpenPopover {
        fn model(&self) -> Option<Documents> {
            if let Some(model) = self.model.model() {
                model.downcast().ok()
            } else {
                None
            }
        }

        fn set_model(&self, model: Option<&Documents>) {
            self.model.set_model(model);
        }
    }

    impl WidgetImpl for OpenPopover {}
    impl PopoverImpl for OpenPopover {}
}

glib::wrapper! {
    pub struct OpenPopover(ObjectSubclass<imp::OpenPopover>)
        @extends gtk::Widget, gtk::Popover,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager,
            gtk::Native;
}

impl OpenPopover {
    pub fn new<P: IsA<Documents>>(model: &P) -> Self {
        glib::Object::builder().property("model", model).build()
    }

    /// Connect to the signal emitted when a user clicks a document in the document list.
    pub fn connect_document_activated<F: Fn(&Self, &Document) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "document-activated",
            true,
            closure_local!(move |obj: Self, document: Document| {
                f(&obj, &document);
            }),
        )
    }
}
