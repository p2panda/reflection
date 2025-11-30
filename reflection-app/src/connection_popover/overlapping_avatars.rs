/* overlapping_avatars.rs
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
use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, glib};

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct OverlappingAvatars {}

    #[glib::object_subclass]
    impl ObjectSubclass for OverlappingAvatars {
        const NAME: &'static str = "ReflectionOverlappingAvatars";
        type Type = super::OverlappingAvatars;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::List);
            klass.set_css_name("overlapping-avatars");
        }
    }

    impl ObjectImpl for OverlappingAvatars {}

    impl WidgetImpl for OverlappingAvatars {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let mut child = self.obj().last_child();

            while let Some(widget) = child {
                let prev_widget = widget.prev_sibling();
                if let Some(ref prev_widget) = prev_widget
                    && let Some(rect) = prev_widget.compute_bounds(self.obj().as_ref()) {
                        snapshot.push_mask(gtk::gsk::MaskMode::InvertedAlpha);
                        // This only works for circular widgets like avatars,
                        // maybe we should just use the widget as mask so it works for all widgets?
                        let round_rect = gtk::gsk::RoundedRect::from_rect(rect, 9999.9);
                        snapshot.push_rounded_clip(&round_rect);
                        snapshot.append_color(&gdk::RGBA::BLACK, &rect);
                        // Pop rounded clip
                        snapshot.pop();
                        // Finish creating the mask
                        snapshot.pop();
                    }

                self.obj().snapshot_child(&widget, snapshot);
                if prev_widget.is_some() {
                    // Pop mask in case it was added
                    snapshot.pop();
                }
                child = prev_widget;
            }
        }
    }

    impl BoxImpl for OverlappingAvatars {}
    impl AccessibleImpl for OverlappingAvatars {}
}

glib::wrapper! {
    pub struct OverlappingAvatars(ObjectSubclass<imp::OverlappingAvatars>)
        @extends gtk::Widget, gtk::Box,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl OverlappingAvatars {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
