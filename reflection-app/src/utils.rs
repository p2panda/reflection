use formatx::formatx;
use gettextrs::gettext;
use gio::prelude::{MenuLinkIterExt, MenuModelExt};
use gtk::{gio, glib};

use crate::ReflectionApplication;
use crate::system_settings::ClockFormat;

// This was copied from Fractal
// See: https://gitlab.gnome.org/World/fractal/-/blob/main/src/session/model/user_sessions_list/user_session.rs#L258
pub fn format_datetime(last_string: &str, datetime: &glib::DateTime) -> String {
    let clock_format = ReflectionApplication::default()
        .system_settings()
        .clock_format();
    let use_24 = clock_format == ClockFormat::TwentyFourHours;

    // This was ported from Nautilus and simplified for our use case.
    // See: https://gitlab.gnome.org/GNOME/nautilus/-/blob/1c5bd3614a35cfbb49de087bc10381cdef5a218f/src/nautilus-file.c#L5001
    let now = glib::DateTime::now_utc().unwrap();
    let format;
    let days_ago = {
        let today_midnight =
            glib::DateTime::from_utc(now.year(), now.month(), now.day_of_month(), 0, 0, 0f64)
                .expect("constructing GDateTime works");

        let date = glib::DateTime::from_utc(
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
            // Translators: Time in 24h format, i.e. "{last_string} at 23:04".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            format = formatx!(gettext("{last_string} at %H:%M"), last_string = last_string)
                .expect("Valid format string");
        } else {
            // Translators: Time in 12h format, i.e. "{last_string} at 11:04 PM".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            format = formatx!(
                gettext("{last_string} at %I:%M %p"),
                last_string = last_string
            )
            .expect("Valid format string");
        }
    }
    // Show the word "Yesterday" and time if date is on yesterday
    else if days_ago == 1 {
        if use_24 {
            // Translators: this a time in 24h format, i.e. "{last_string} yesterday at 23:04".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = formatx!(
                gettext("{last_string} yesterday at %H:%M"),
                last_string = last_string
            )
            .expect("Valid format string");
        } else {
            // Translators: this is a time in 12h format, i.e. "{last_string} Yesterday at 11:04
            // PM".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = formatx!(
                gettext("{last_string} yesterday at %I:%M %p"),
                last_string = last_string
            )
            .expect("Valid format string");
        }
    }
    // Show a week day and time if date is in the last week
    else if days_ago > 1 && days_ago < 7 {
        if use_24 {
            // Translators: this is the name of the week day followed by a time in 24h
            // format, i.e. "{last_string} Monday at 23:04".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            //  See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = formatx!(
                gettext("{last_string} %A at %H:%M"),
                last_string = last_string
            )
            .expect("Valid format string");
        } else {
            // Translators: this is the week day name followed by a time in 12h format, i.e.
            // "{last_string} Monday at 11:04 PM".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = formatx!(
                gettext("{last_string} %A at %I:%M %p"),
                last_string = last_string
            )
            .expect("Valid format string");
        }
    } else if datetime.year() == now.year() {
        if use_24 {
            // Translators: this is the month and day and the time in 24h format, i.e. "{last_string} February 3 at 23:04".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = formatx!(
                gettext("{last_string} %B %-e at %H:%M"),
                last_string = last_string
            )
            .expect("Valid format string");
        } else {
            // Translators: this is the month and day and the time in 12h format, i.e. "{last_string} February 3 at 11:04 PM".
            // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
            // Do not change the time format as it will follow the system settings.
            // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
            // xgettext:no-c-format
            format = formatx!(
                gettext("{last_string} %B %-e at %I:%M %p"),
                last_string = last_string
            )
            .expect("Valid format string");
        }
    } else if use_24 {
        // Translators: this is the full date and the time in 24h format, i.e. "{last_string} February 3 2015 at 23:04".
        // {last_string} will be replaced with "Last seen", "Last online", "Last accessed" or similar.
        // Do not change the time format as it will follow the system settings.
        // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
        // xgettext:no-c-format
        format = formatx!(
            gettext("{last_string} %B %-e %Y at %H:%M"),
            last_string = last_string
        )
        .expect("Valid format string");
    } else {
        // Translators: this is the full date and the time in 12h format, i.e. "{last_string} February 3 2015 at 11:04 PM".
        // Do not change the time format as it will follow the system settings.
        // See `man strftime` or the documentation of g_date_time_format for the available specifiers: <https://docs.gtk.org/glib/method.DateTime.format.html>
        // xgettext:no-c-format
        format = formatx!(
            gettext("{last_string} %B %-e %Y at %I:%M %p"),
            last_string = last_string
        )
        .expect("Valid format string");
    }

    datetime
        .to_local()
        .expect("constructing local GDateTime works")
        .format(&format)
        .expect("formatting GDateTime works")
        .into()
}

/// Sets the given `target` as an action target for all entries with an action
pub fn menu_set_action_target(
    menu_model: &gio::MenuModel,
    target: Option<&glib::Variant>,
) -> gio::MenuModel {
    let menu = gio::Menu::new();
    for i in 0..menu_model.n_items() {
        let item = gio::MenuItem::from_model(menu_model, i);
        let link_iter = menu_model.iterate_item_links(i);
        while let Some((name, menu_model)) = link_iter.next() {
            let link_menu = menu_set_action_target(&menu_model, target);
            item.set_link(&name, Some(&link_menu));
        }

        if item
            .attribute_value(gio::MENU_ATTRIBUTE_ACTION, None)
            .is_some()
        {
            item.set_attribute_value(gio::MENU_ATTRIBUTE_TARGET, target);
        }

        menu.append_item(&item);
    }

    menu.into()
}
