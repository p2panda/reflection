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

use adw::prelude::ActionRowExt;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;

use crate::ReflectionApplication;
use crate::components::Avatar;
use crate::system_settings::ClockFormat;
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
                let row = adw::ActionRow::builder()
                    .selectable(false)
                    .activatable(false)
                    .can_focus(false)
                    .can_target(false)
                    .build();
                let avatar = Avatar::new();
                row.add_prefix(&avatar);
                if author.is_this_device() {
                    let this_device_label = gtk::Label::builder()
                        .label("This Device")
                        .valign(gtk::Align::Start)
                        .margin_top(6)
                        .css_classes(["this-device-pill"])
                        .build();
                    row.add_suffix(&this_device_label);
                }
                author
                    .bind_property("name", &row, "title")
                    .sync_create()
                    .build();
                // FIXME: format last seen according to the mockups
                //author.bind_property ("last-seen", row, "subtitle").sync_create().build();
                author
                    .bind_property("emoji", &avatar, "emoji")
                    .sync_create()
                    .build();
                author
                    .bind_property("is-online", &row, "subtitle")
                    .sync_create()
                    .transform_to(|binding, is_online: bool| {
                        let author: Author = binding.source().unwrap().downcast().unwrap();
                        if is_online {
                            Some("Online".to_string())
                        } else if let Some(last_seen) = author.last_seen() {
                            Some(format_last_seen(&last_seen))
                        } else {
                            Some("Never seen".to_string())
                        }
                    })
                    .build();
                avatar.add_css_class(&format!("bg-{}", author.color()));

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
        @extends gtk::Widget, adw::Bin;
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

// This was copied from Fractal
// See: https://gitlab.gnome.org/World/fractal/-/blob/main/src/session/model/user_sessions_list/user_session.rs#L258
fn format_last_seen(datetime: &glib::DateTime) -> String {
    let clock_format = ReflectionApplication::default()
        .system_settings()
        .clock_format();
    let use_24 = clock_format == ClockFormat::TwentyFourHours;

    // This was ported from Nautilus and simplified for our use case.
    // See: https://gitlab.gnome.org/GNOME/nautilus/-/blob/1c5bd3614a35cfbb49de087bc10381cdef5a218f/src/nautilus-file.c#L5001
    let now = glib::DateTime::now_local().unwrap();
    let format;
    let days_ago = {
        let today_midnight =
            glib::DateTime::from_local(now.year(), now.month(), now.day_of_month(), 0, 0, 0f64)
                .expect("constructing GDateTime works");

        let date = glib::DateTime::from_local(
            datetime.year(),
            datetime.month(),
            datetime.day_of_month(),
            0,
            0,
            0f64,
        )
        .expect("constructing GDateTime works");

        today_midnight.difference(&date).as_days()
    };

    // Show only the time if date is on today
    if days_ago == 0 {
        if use_24 {
            // Translators: Time in 24h format, i.e. "23:04".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            format = gettext("Last seen at %H:%M");
        } else {
            // Translators: Time in 12h format, i.e. "11:04 PM".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            format = gettext("Last seen at %I:%M %p");
        }
    }
    // Show the word "Yesterday" and time if date is on yesterday
    else if days_ago == 1 {
        if use_24 {
            // Translators: this a time in 24h format, i.e. "Last seen yesterday at 23:04".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = gettext("Last seen yesterday at %H:%M");
        } else {
            // Translators: this is a time in 12h format, i.e. "Last seen Yesterday at 11:04
            // PM".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = gettext("Last seen yesterday at %I:%M %p");
        }
    }
    // Show a week day and time if date is in the last week
    else if days_ago > 1 && days_ago < 7 {
        if use_24 {
            // Translators: this is the name of the week day followed by a time in 24h
            // format, i.e. "Last seen Monday at 23:04".
            // Do not change the time format as it will follow the system settings.
            //  See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = gettext("Last seen %A at %H:%M");
        } else {
            // Translators: this is the week day name followed by a time in 12h format, i.e.
            // "Last seen Monday at 11:04 PM".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = gettext("Last seen %A at %I:%M %p");
        }
    } else if datetime.year() == now.year() {
        if use_24 {
            // Translators: this is the month and day and the time in 24h format, i.e. "Last
            // seen February 3 at 23:04".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = gettext("Last seen %B %-e at %H:%M");
        } else {
            // Translators: this is the month and day and the time in 12h format, i.e. "Last
            // seen February 3 at 11:04 PM".
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = gettext("Last seen %B %-e at %I:%M %p");
        }
    } else if use_24 {
        // Translators: this is the full date and the time in 24h format, i.e. "Last
        // seen February 3 2015 at 23:04".
        // Do not change the time format as it will follow the system settings.
        // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
        // xgettext:no-c-format
        format = gettext("Last seen %B %-e %Y at %H:%M");
    } else {
        // Translators: this is the full date and the time in 12h format, i.e. "Last
        // seen February 3 2015 at 11:04 PM".
        // Do not change the time format as it will follow the system settings.
        // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
        // xgettext:no-c-format
        format = gettext("Last seen %B %-e %Y at %I:%M %p");
    }

    datetime
        .format(&format)
        .expect("formatting GDateTime works")
        .into()
}
