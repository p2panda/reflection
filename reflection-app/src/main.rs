/* main.rs
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

mod application;
mod components;
mod config;
mod connection_popover;
mod document_view;
mod error_page;
mod landing_view;
#[cfg(target_os = "macos")]
mod macos_bundle;
mod open_dialog;
mod secret;
mod system_settings;
mod textbuffer;
mod textview;
mod utils;
mod window;

use std::path::PathBuf;

use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::prelude::*;
use gtk::{gio, glib};
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

use self::application::ReflectionApplication;
use self::config::*;
use self::connection_popover::ConnectionPopover;
use self::textbuffer::ReflectionTextBuffer;
use self::textview::TextView;

pub use self::config::APP_ID;

fn main() -> glib::ExitCode {
    // This changes the environment, so it needs to run before anything else
    let (localedir, pkgdatadir) = data_dirs();

    setup_logging();

    // Set up gettext translations
    bindtextdomain(GETTEXT_PACKAGE, localedir).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let res = gio::Resource::load(pkgdatadir.join("resources.gresource"))
        .expect("Could not load gresource file");
    gio::resources_register(&res);
    let ui_res = gio::Resource::load(pkgdatadir.join("ui-resources.gresource"))
        .expect("Could not load UI gresource file");
    gio::resources_register(&ui_res);

    #[cfg(target_os = "macos")]
    {
        let mac_res = gio::Resource::load(pkgdatadir.join("mac-resources.gresource"))
            .expect("Could not load MacOS gresource file");
        gio::resources_register(&mac_res);
    }

    gtk::glib::set_application_name("Reflection");
    gtk::init().expect("Could not start GTK4");

    info!("Reflection ({})", APP_ID);
    info!("Version: {}", VERSION);
    info!("Datadir: {}", pkgdatadir.display());

    let app = ReflectionApplication::new();
    app.run()
}

/// Returns the directories containing the translations and the application data.
fn data_dirs() -> (PathBuf, PathBuf) {
    #[cfg(target_os = "macos")]
    if let Some(bundle) = macos_bundle::Bundle::locate() {
        bundle.setup_environment();
        return (bundle.locale_dir(), bundle.pkgdata_dir());
    }

    (PathBuf::from(LOCALEDIR), PathBuf::from(PKGDATADIR))
}

fn setup_logging() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env())
        .try_init()
        .ok();
}
