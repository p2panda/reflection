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
use gtk::glib;
use gtk::prelude::*;

use crate::components::IndicatorBin;
use crate::utils::format_datetime;
use reflection_doc::author::Author;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::AuthorRow)]
    #[template(file = "src/connection_popover/author_row.blp")]
    pub struct AuthorRow {
        #[property(get, set)]
        author: RefCell<Option<Author>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AuthorRow {
        const NAME: &'static str = "ReflectionAuthorRow";
        type Type = super::AuthorRow;
        type ParentType = gtk::ListBoxRow;

        fn class_init(klass: &mut Self::Class) {
            IndicatorBin::static_type();
            klass.bind_template();
            klass.bind_template_callbacks();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl AuthorRow {
        #[template_callback]
        fn format_subtitle(&self) -> Option<String> {
            let author_borrow = self.author.borrow();
            let author = author_borrow.as_ref()?;

            if author.is_online() {
                Some(gettext("Online"))
            } else if let Some(last_seen) = author.last_seen() {
                if author.is_this_device() {
                    Some(format_datetime(&gettext("Last online"), &last_seen))
                } else {
                    Some(format_datetime(&gettext("Last seen"), &last_seen))
                }
            } else if author.is_this_device() {
                Some(gettext("Offline"))
            } else {
                Some(gettext("Never seen"))
            }
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AuthorRow {}

    impl WidgetImpl for AuthorRow {}
    impl ListBoxRowImpl for AuthorRow {}
}

glib::wrapper! {
    pub struct AuthorRow(ObjectSubclass<imp::AuthorRow>)
        @extends gtk::Widget, gtk::ListBoxRow,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl AuthorRow {
    pub fn new<P: IsA<Author>>(author: Option<&P>) -> Self {
        glib::Object::builder().property("author", author).build()
    }
}
