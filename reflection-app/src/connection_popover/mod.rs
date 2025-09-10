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

use std::marker::PhantomData;

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;

use reflection_doc::authors::Authors;

mod author_list;
use self::author_list::AuthorList;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::ConnectionPopover)]
    pub struct ConnectionPopover {
        author_list: AuthorList,
        #[property(get = Self::authors, set = Self::set_authors)]
        authors: PhantomData<Option<Authors>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ConnectionPopover {
        const NAME: &'static str = "ReflectionConnectionPopover";
        type Type = super::ConnectionPopover;
        type ParentType = gtk::Popover;
    }

    #[glib::derived_properties]
    impl ObjectImpl for ConnectionPopover {
        fn constructed(&self) {
            let scrollview = gtk::ScrolledWindow::builder()
                .child(&self.author_list)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .propagate_natural_width(true)
                .max_content_height(300)
                .build();
            self.obj().set_child(Some(&scrollview));
            self.obj().add_css_class("connection-popover");
        }
    }

    impl ConnectionPopover {
        fn set_authors(&self, authors: Option<Authors>) {
            self.author_list.set_model(authors);
        }

        fn authors(&self) -> Option<Authors> {
            self.author_list.model()
        }
    }

    impl WidgetImpl for ConnectionPopover {}
    impl PopoverImpl for ConnectionPopover {}
}

glib::wrapper! {
    pub struct ConnectionPopover(ObjectSubclass<imp::ConnectionPopover>)
        @extends gtk::Widget, gtk::Popover;
}

impl ConnectionPopover {
    pub fn new<P: IsA<Authors>>(authors: &P) -> Self {
        glib::Object::builder().property("authors", authors).build()
    }
}
