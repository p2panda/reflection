/* zoom_level_selector.rs
 *
 * Copyright 2025 Julian Sparber <julian@sparber.net>
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
use std::cell::Cell;

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use sourceview::*;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::ZoomLevelSelector)]
    #[template(file = "src/components/zoom_level_selector.blp")]
    pub struct ZoomLevelSelector {
        #[template_child]
        pub button: TemplateChild<gtk::Button>,
        #[property(get, set)]
        zoom_level: Cell<f64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ZoomLevelSelector {
        const NAME: &'static str = "ZoomLevelSelector";
        type Type = super::ZoomLevelSelector;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ZoomLevelSelector {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj()
                .bind_property("zoom_level", &*self.button, "label")
                .sync_create()
                .transform_to(|_, zoom_level: f64| Some(format!("{:.0}%", zoom_level * 100.0)))
                .build();
        }
    }

    impl WidgetImpl for ZoomLevelSelector {}
    impl BoxImpl for ZoomLevelSelector {}
}

glib::wrapper! {
    pub struct ZoomLevelSelector(ObjectSubclass<imp::ZoomLevelSelector>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ZoomLevelSelector {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }
}

