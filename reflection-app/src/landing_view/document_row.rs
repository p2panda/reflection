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

use std::cell::RefCell;

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib, glib::clone};

use crate::utils::format_datetime;
use crate::utils::menu_set_action_target;
use reflection_doc::document::{Document, DocumentId};

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::DocumentRow)]
    #[template(file = "src/landing_view/document_row.blp")]
    pub struct DocumentRow {
        #[property(get, set)]
        document: RefCell<Option<Document>>,

        #[template_child]
        menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        menu_model: TemplateChild<gio::MenuModel>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DocumentRow {
        const NAME: &'static str = "ReflectionDocumentRow";
        type Type = super::DocumentRow;
        type ParentType = adw::ActionRow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
            klass.set_css_name("row");
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for DocumentRow {
        fn constructed(&self) {
            self.parent_constructed();

            self.menu_button.set_create_popup_func(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    this.update_menu_model();
                }
            ));
        }
    }

    #[gtk::template_callbacks(functions)]
    impl DocumentRow {
        #[template_callback]
        fn transform_action_target(id: Option<DocumentId>) -> glib::Variant {
            if let Some(id) = id {
                [id].to_variant()
            } else {
                let list: &[DocumentId] = &[];
                list.to_variant()
            }
        }

        #[template_callback]
        fn transform_name(name: Option<&str>) -> String {
            if let Some(name) = name {
                name.to_string()
            } else {
                gettext("Empty Pad")
            }
        }

        #[template_callback]
        fn transform_last_accessed(
            last_accessed: Option<glib::DateTime>,
            subscribed: bool,
        ) -> String {
            if let Some(last_accessed) = last_accessed {
                format_datetime(&gettext("Last accessed"), &last_accessed)
            } else if subscribed {
                gettext("Currently open")
            } else {
                gettext("Never accessed")
            }
        }

        fn update_menu_model(&self) {
            let Some(ref document) = *self.document.borrow() else {
                return;
            };

            let target = Self::transform_action_target(Some(document.id()));
            let menu = menu_set_action_target(&self.menu_model, Some(&target));
            self.menu_button.set_menu_model(Some(&menu));
        }
    }

    impl WidgetImpl for DocumentRow {}
    impl ListBoxRowImpl for DocumentRow {}
    impl PreferencesRowImpl for DocumentRow {}
    impl ActionRowImpl for DocumentRow {}
}

glib::wrapper! {
    pub struct DocumentRow(ObjectSubclass<imp::DocumentRow>)
        @extends gtk::Widget, gtk::ListBoxRow, adw::ActionRow, adw::PreferencesRow,
        @implements gtk::Accessible, gtk::Buildable, gtk::Actionable, gtk::ConstraintTarget;
}

impl DocumentRow {
    pub fn new(document: Option<&Document>) -> Self {
        glib::Object::builder()
            .property("document", document)
            .build()
    }
}
