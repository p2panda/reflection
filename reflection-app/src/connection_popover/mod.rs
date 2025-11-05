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

use std::cell::RefCell;

use adw::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib, glib::clone};

use reflection_doc::{document::Document, service::ConnectionMode};

mod author_list;
mod authors_stack;
mod overlapping_avatars;

use self::author_list::AuthorList;
use self::authors_stack::AuthorsStack;

mod imp {
    use super::*;

    #[derive(Debug, Default, glib::Properties, gtk::CompositeTemplate)]
    #[properties(wrapper_type = super::ConnectionPopover)]
    #[template(resource = "/org/p2panda/reflection/connection_popover/connection_popover.ui")]
    pub struct ConnectionPopover {
        #[template_child]
        button_icon_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        author_list: TemplateChild<AuthorList>,
        #[template_child]
        connection_mode_switch: TemplateChild<adw::ToggleGroup>,
        #[template_child]
        no_network_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        network_toggle_image: TemplateChild<gtk::Image>,
        #[property(get, set = Self::set_document)]
        document: RefCell<Option<Document>>,
        #[property(get, set)]
        popover: RefCell<Option<Document>>,
        connection_mode_binding: RefCell<Option<glib::Binding>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ConnectionPopover {
        const NAME: &'static str = "ReflectionConnectionPopover";
        type Type = super::ConnectionPopover;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            AuthorList::static_type();
            AuthorsStack::static_type();
            klass.bind_template();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for ConnectionPopover {
        fn constructed(&self) {
            self.parent_constructed();

            let monitor = gio::NetworkMonitor::default();
            monitor.connect_network_available_notify(clone!(
                #[weak(rename_to = this)]
                self,
                move |_| {
                    this.update_no_network_revealer();
                }
            ));

            self.connection_mode_switch
                .connect_active_name_notify(clone!(
                    #[weak(rename_to = this)]
                    self,
                    move |_| {
                        this.update_no_network_revealer();
                    }
                ));

            self.update_no_network_revealer();
        }
    }

    impl ConnectionPopover {
        fn set_document(&self, document: Option<Document>) {
            if let Some(binding) = self.connection_mode_binding.take() {
                binding.unbind();
            }

            let Some(document) = document else {
                self.document.replace(document);
                return;
            };

            self.author_list.set_model(Some(document.authors()));

            let binding = document
                .service()
                .bind_property(
                    "connection-mode",
                    &self.connection_mode_switch.get(),
                    "active-name",
                )
                .sync_create()
                .bidirectional()
                .transform_to(|_, mode| {
                    let active_name = match mode {
                        ConnectionMode::None => "offline",
                        ConnectionMode::Bluetooth => "bluetooth",
                        ConnectionMode::Network => "network",
                    };
                    Some(active_name)
                })
                .transform_from(|_, active_name| {
                    let mode = match active_name {
                        "offline" => ConnectionMode::None,
                        "bluetooth" => ConnectionMode::Bluetooth,
                        "network" => ConnectionMode::Network,
                        _ => return None,
                    };
                    Some(mode)
                })
                .build();

            self.connection_mode_binding.replace(Some(binding));
            self.document.replace(Some(document));
        }

        fn update_no_network_revealer(&self) {
            let monitor = gio::NetworkMonitor::default();
            let wants_network = self
                .connection_mode_switch
                .active_name()
                .map_or(false, |name| name.as_str() == "network");
            let is_offline = !monitor.is_network_available() && wants_network;

            self.no_network_revealer.set_reveal_child(is_offline);
            if monitor.is_network_available() {
                self.network_toggle_image
                    .set_icon_name(Some("network-symbolic"));
            } else {
                self.network_toggle_image
                    .set_icon_name(Some("no-network-symbolic"));
            }

            if let Some(mode) = self.connection_mode_switch.active_name() {
                let page_name = match mode.as_str() {
                    "offline" => Some("offline"),
                    "network" if !monitor.is_network_available() => Some("no-network"),
                    "network" => Some("network"),
                    _ => None,
                };
                if let Some(page_name) = page_name {
                    self.button_icon_stack.set_visible_child_name(page_name);
                }
            }
        }
    }

    impl WidgetImpl for ConnectionPopover {}
    impl BinImpl for ConnectionPopover {}
}

glib::wrapper! {
    pub struct ConnectionPopover(ObjectSubclass<imp::ConnectionPopover>)
        @extends gtk::Widget, adw::Bin,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl ConnectionPopover {
    pub fn new<P: IsA<Document>>(document: &P) -> Self {
        glib::Object::builder()
            .property("document", document)
            .build()
    }
}
