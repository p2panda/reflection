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

use gtk::glib;
use gtk::subclass::prelude::*;
use sourceview::subclass::prelude::ViewImpl;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct TextView {}

    #[glib::object_subclass]
    impl ObjectSubclass for TextView {
        const NAME: &'static str = "ReflectionTextView";
        type Type = super::TextView;
        type ParentType = sourceview::View;
    }

    impl ObjectImpl for TextView {}
    impl WidgetImpl for TextView {}
    impl TextViewImpl for TextView {}
    impl ViewImpl for TextView {}
}

glib::wrapper! {
    pub struct TextView(ObjectSubclass<imp::TextView>)
        @extends gtk::Widget, gtk::TextView, sourceview::View;
}

impl TextView {}
