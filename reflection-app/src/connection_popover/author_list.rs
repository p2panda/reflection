/* author_list.rs
 *
 * Copyright 2025 The Reflection Developers
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
use gtk::glib;
use gtk::prelude::*;

use crate::connection_popover::author_row::AuthorRow;
use reflection_doc::{author::Author, author::COLORS, authors::Authors};

mod imp {
    use super::*;
    use adw::prelude::BinExt;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::AuthorList)]
    pub struct AuthorList {
        list_box: gtk::ListBox,
        #[property(get, set = Self::set_model, nullable)]
        model: RefCell<Option<Authors>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AuthorList {
        const NAME: &'static str = "ReflectionAuthorList";
        type Type = super::AuthorList;
        type ParentType = adw::Bin;
    }

    #[glib::derived_properties]
    impl ObjectImpl for AuthorList {
        fn constructed(&self) {
            self.obj().set_child(Some(&self.list_box));
            self.list_box.set_selection_mode(gtk::SelectionMode::None);

            let css_provider = gtk::CssProvider::new();
            let style: String = COLORS
                .iter()
                .map(|(color_name, color_hex)| {
                    format!(".bg-{color_name} {{ background-color: {color_hex}; }}")
                })
                .collect();
            css_provider.load_from_string(&style);
            gtk::style_context_add_provider_for_display(
                &self.obj().display(),
                &css_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    impl AuthorList {
        fn set_model(&self, model: Option<Authors>) {
            self.list_box.bind_model(model.as_ref(), |author| {
                let author = author.downcast_ref::<Author>().unwrap();
                let row = AuthorRow::new(Some(author));

                row.upcast()
            });

            self.model.replace(model);
        }
    }

    impl WidgetImpl for AuthorList {}
    impl BinImpl for AuthorList {}
}

glib::wrapper! {
    pub struct AuthorList(ObjectSubclass<imp::AuthorList>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AuthorList {
    pub fn new<P: IsA<Authors>>(model: &P) -> Self {
        glib::Object::builder().property("model", model).build()
    }
}

impl Default for AuthorList {
    fn default() -> Self {
        glib::Object::new()
    }
}
