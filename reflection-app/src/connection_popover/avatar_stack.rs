/* avatar_stack.rs
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
use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gdk, glib, glib::clone};
use reflection_doc::{author::Author, authors::Authors};

use crate::components::Avatar;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::AvatarStack)]
    pub struct AvatarStack {
        #[property(get, set = Self::set_model, nullable)]
        model: RefCell<Option<Authors>>,
        #[property(get, set)]
        no_authors_visible: Cell<u32>,
        children: RefCell<Vec<Avatar>>,
        arrow: gtk::Image,
        overflow_label: gtk::Label,
        items_changed_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    impl AvatarStack {
        fn set_model(&self, model: Option<Authors>) {
            if let Some(model) = self.model.take() {
                if let Some(items_changed_handler) = self.items_changed_handler.take() {
                    model.disconnect(items_changed_handler);
                }

                let mut children = self.children.borrow_mut();
                for child in children.drain(..) {
                    child.unparent();
                }
            }

            let Some(model) = model else {
                return;
            };

            let handler = model.connect_items_changed(clone!(
                #[weak(rename_to = this)]
                self,
                move |model, position, removed, added| {
                    let mut children = this.children.borrow_mut();
                    let removed_children =
                        children.drain(position as usize..(position + removed) as usize);
                    for child in removed_children {
                        child.unparent();
                    }

                    for index in position..(position + added) {
                        if let Some(item) = model.item(index) {
                            let author = item.downcast::<Author>().unwrap();
                            this.add_author(author);
                        }
                    }

                    this.update_arrow_visibility();
                    this.update_overflow_label_visibility();
                }
            ));

            for author in model.iter::<Author>() {
                self.add_author(author.unwrap());
            }

            self.items_changed_handler.replace(Some(handler));
            self.model.replace(Some(model));
            self.update_arrow_visibility();
            self.update_overflow_label_visibility();
        }

        fn add_author(&self, author: Author) {
            let mut children = self.children.borrow_mut();
            let avatar = Avatar::new();
            //FIXME: We need to listen for changes in author color property,
            // or mark the property as construction only
            avatar.add_css_class(&format!("bg-{}", author.color()));
            author
                .bind_property("emoji", &avatar, "emoji")
                .sync_create()
                .build();
            avatar.set_parent(self.obj().as_ref());
            children.push(avatar);
        }

        fn update_arrow_visibility(&self) {
            // We show a drop down arrow when without any remote authors
            let children = self.children.borrow();
            if children.len() <= 1 {
                if let Some(child) = children.get(0) {
                    child.set_visible(false);
                }
                self.arrow.set_visible(true);
            } else {
                if let Some(child) = children.get(0) {
                    child.set_visible(true);
                }
                self.arrow.set_visible(false);
            }
        }

        fn update_overflow_label_visibility(&self) {
            // We show a drop down arrow when without any remote authors
            let model = self.model.borrow();
            if let Some(ref model) = *model {
                let n_items = model.n_items();
                self.overflow_label.set_visible(n_items > 4);
                                self.overflow_label.set_visible(true);
                self.overflow_label.set_label(&format!("+ {n_items}"));
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AvatarStack {
        const NAME: &'static str = "ReflectionAvatarStack";
        type Type = super::AvatarStack;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AvatarStack {
        fn constructed(&self) {
            self.parent_constructed();
            self.arrow.set_icon_name(Some("pan-down-symbolic"));
            self.arrow.set_parent(self.obj().as_ref());
            self.arrow.set_visible(false);
            self.overflow_label.set_visible(true);
            self.overflow_label.set_valign(gtk::Align::Center);
            self.overflow_label.set_parent(self.obj().as_ref());
            self.overflow_label.add_css_class("caption");
            self.overflow_label.add_css_class("authors-overflow");
        }

        fn dispose(&self) {
            self.arrow.unparent();
            let mut children = self.children.borrow_mut();
            for child in children.drain(..) {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for AvatarStack {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let children = self.children.borrow();
            if children.is_empty() || !children.iter().any(|child| child.is_visible()) {
                return self.arrow.measure(orientation, for_size);
            }

            let mut sum_min = 0;
            let mut sum_nat = 0;
            if orientation == gtk::Orientation::Vertical {
            } else {
                //sum_min -= 24;
                //sum_nat -= 24;
            }
            for child in children.iter() {
                let (min, nat, _, _) = child.measure(orientation, for_size);
                if orientation == gtk::Orientation::Vertical {
                    sum_min = std::cmp::max(min, sum_min);
                    sum_nat = std::cmp::max(nat, sum_nat);
                } else {
                    println!(
                        "In measure (min: {min}, nat: {nat}) {orientation:?} for_size: {for_size}"
                    );
                    sum_min += min;
                    sum_nat += nat;
                }
            }
            println!("({sum_min}, {sum_nat}) {orientation:?} ");

            let (overflow_label_min, overflow_label_nat, _, _) =
                self.overflow_label.measure(orientation, for_size);
            sum_min += overflow_label_min;
            sum_nat += overflow_label_nat;

            return (sum_min, sum_nat, -1, -1);
            /*if orientation == gtk::Orientation::Vertical {
                return (size, size, -1, -1);
            }

            let n_children = u32::try_from(self.children.borrow().len())
                .expect("count of children fits into u32");

            // The last avatar has no overlap.
            let mut size = n_children.saturating_sub(1) * self.distance_between_centers();
            size += avatar_size;

            let size = size.try_into().unwrap_or(i32::MAX);
            (size, size, -1, -1)
            */
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let (min, _, _, _) = self.arrow.measure(gtk::Orientation::Horizontal, height);
            let allocation = gdk::Rectangle::new(0, 0, min, height);
            self.arrow.size_allocate(&allocation, -1);
            let mut x = 0;
            for child in self.children.borrow().iter().rev() {
                let (min, _, _, _) = child.measure(gtk::Orientation::Horizontal, height);
                let allocation = gdk::Rectangle::new(x, 0, min, height);
                child.size_allocate(&allocation, -1);

                x = x.saturating_add(8);
            }

            let (min, _, _, _) = self.overflow_label.measure(gtk::Orientation::Horizontal, height);
            let allocation = gdk::Rectangle::new(width - min, 0, min, height);
            self.overflow_label.size_allocate(&allocation, -1);
        }
    }

    impl AccessibleImpl for AvatarStack {
        fn first_accessible_child(&self) -> Option<gtk::Accessible> {
            // Hide the children in the a11y tree.
            None
        }
    }
}

glib::wrapper! {
    pub struct AvatarStack(ObjectSubclass<imp::AvatarStack>)
        @extends gtk::Widget;
}

impl AvatarStack {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
