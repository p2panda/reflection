/* authors_stack.rs
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
use gtk::prelude::*;
use gtk::{glib, glib::clone};
use reflection_doc::{author::Author, authors::Authors};

use crate::components::Avatar;
use crate::connection_popover::overlapping_avatars::OverlappingAvatars;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::AuthorsStack)]
    #[template(file = "src/connection_popover/authors_stack.blp")]
    pub struct AuthorsStack {
        #[template_child]
        main_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        avatars_box: TemplateChild<gtk::Box>,
        #[template_child]
        avatars: TemplateChild<OverlappingAvatars>,
        #[template_child]
        overflow_label: TemplateChild<gtk::Label>,
        #[template_child]
        arrow_image: TemplateChild<gtk::Image>,
        #[property(get = Self::model, set = Self::set_model, nullable, type = Option<Authors>)]
        model: gtk::FilterListModel,
        #[property(get, set = Self::set_max_visible)]
        max_visible: Cell<u32>,
        #[property(get, set = Self::set_show_offline)]
        show_offline: Cell<bool>,
        filter: gtk::EveryFilter,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AuthorsStack {
        const NAME: &'static str = "ReflectionAuthorsStack";
        type Type = super::AuthorsStack;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            OverlappingAvatars::static_type();
            klass.bind_template();
            klass.set_css_name("authors-stack");
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl AuthorsStack {
        fn set_max_visible(&self, max_visible: u32) {
            self.max_visible.set(max_visible);
            self.update_avatars();
        }

        fn set_show_offline(&self, show_offline: bool) {
            if show_offline {
                self.model.set_filter(None::<&gtk::Filter>);
            } else {
                self.model.set_filter(Some(&self.filter));
            }

            self.show_offline.set(show_offline);
        }

        fn model(&self) -> Option<Authors> {
            self.model.model()?.downcast().ok()
        }

        fn set_model(&self, model: Option<Authors>) {
            self.model.set_model(model.as_ref());
        }

        fn create_avatar(&self) -> Avatar {
            let avatar = Avatar::new(None);
            self.avatars.append(&avatar);
            avatar
        }

        fn update_avatars(&self) {
            let max_visible = self.max_visible.get();
            let mut child = self.avatars.first_child();
            for index in 0..max_visible {
                let avatar = if let Some(widget) = child {
                    
                    widget.downcast::<Avatar>().unwrap()
                } else {
                    self.create_avatar()
                };

                let author: Option<Author> =
                    self.model.item(index).and_then(|obj| obj.downcast().ok());
                avatar.set_author(author.as_ref());
                avatar.set_visible(author.is_some());

                child = avatar.next_sibling();
            }

            while let Some(widget) = child {
                self.avatars.remove(&widget);
                child = widget.next_sibling();
            }
        }

        fn update_visible_child(&self) {
            let count = self.model.n_items();
            if count > 0 {
                self.main_stack.set_visible_child(&*self.avatars_box);
                let overflow_count = count.saturating_sub(self.obj().max_visible());
                self.overflow_label.set_visible(overflow_count > 0);
                self.overflow_label
                    .set_label(&format!("+ {}", overflow_count));
            } else {
                self.main_stack.set_visible_child(&*self.arrow_image);
            }
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for AuthorsStack {
        fn constructed(&self) {
            self.parent_constructed();

            self.filter.append(
                gtk::BoolFilter::builder()
                    .invert(true)
                    .expression(Author::this_expression("is-this-device"))
                    .build(),
            );
            self.filter.append(
                gtk::BoolFilter::builder()
                    .expression(Author::this_expression("is-online"))
                    .build(),
            );

            self.model.set_watch_items(true);
            self.model.set_filter(Some(&self.filter));

            self.model.connect_items_changed(clone!(
                #[weak(rename_to = this)]
                self,
                move |_model, _position, _removed, _added| {
                    this.update_visible_child();
                    this.update_avatars();
                }
            ));

            self.update_visible_child();
            self.update_avatars();
        }
    }

    impl WidgetImpl for AuthorsStack {}
    impl BinImpl for AuthorsStack {}
    impl AccessibleImpl for AuthorsStack {}
}

glib::wrapper! {
    pub struct AuthorsStack(ObjectSubclass<imp::AuthorsStack>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AuthorsStack {
    pub fn new() -> Self {
        glib::Object::new()
    }
}
