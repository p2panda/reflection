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

use gtk::{
    glib,
    glib::clone,
    prelude::{IsA, ObjectExt},
};

mod document_row;

use document_row::DocumentRow;
use reflection_doc::{document::Document, documents::Documents};

mod imp {
    use super::*;

    use adw::prelude::{Cast, EditableExt, ListModelExt, StaticType, WidgetExt};
    use adw::subclass::prelude::{
        CompositeTemplateClass, CompositeTemplateInitializingExt, NavigationPageImpl,
        WidgetClassExt, WidgetImpl,
    };

    use glib::subclass::prelude::*;
    use gtk::TemplateChild;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::LandingView)]
    #[template(file = "src/landing_view/landing_view.blp")]
    pub struct LandingView {
        #[template_child]
        search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        title: TemplateChild<adw::WindowTitle>,

        #[template_child]
        header_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        document_list_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        listbox: TemplateChild<gtk::ListBox>,
        #[template_child]
        bottom_buttons: TemplateChild<gtk::Box>,

        #[property(get = Self::model, set = Self::set_model, type = Option<Documents>, nullable)]
        model: gtk::FilterListModel,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LandingView {
        const NAME: &'static str = "ReflectionLandingView";
        type Type = super::LandingView;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for LandingView {
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
                move |_model, _, _, _| {
                    this.update_stack();
                }
            ));

            self.listbox.bind_model(Some(&self.model), |item| {
                let document = item.downcast_ref::<Document>().unwrap();
                DocumentRow::new(Some(document)).upcast()
            });

            self.listbox.connect_row_activated(clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _pos| {
                    this.search_entry.set_text("");
                }
            ));

            self.update_stack();
        }
    }

    impl LandingView {
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

        fn update_stack(&self) {
            let n_items = self.model.model().map_or(0, |model| model.n_items());
            let filtered_n_items = self.model.n_items();

            let header_visibile_child = if n_items > 0 {
                "documents"
            } else {
                "no-documents"
            };
            self.header_stack
                .set_visible_child_name(header_visibile_child);

            let visible_child = if filtered_n_items > 0 {
                "documents"
            } else if n_items > 0 {
                "no-matching-documents"
            } else {
                "no-documents"
            };
            self.document_list_stack
                .set_visible_child_name(visible_child);
            self.bottom_buttons.set_visible(n_items > 0);
        }
    }

    impl WidgetImpl for LandingView {}
    impl NavigationPageImpl for LandingView {}
}

glib::wrapper! {
    pub struct LandingView(ObjectSubclass<imp::LandingView>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager,
            gtk::Native;
}

impl LandingView {
    pub fn new<P: IsA<Documents>>(model: &P) -> Self {
        glib::Object::builder().property("model", model).build()
    }
}
