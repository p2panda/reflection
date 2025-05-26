/* main.rs
 *
 * Copyright 2024 The Aardvark Developers
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
mod open_dialog;
mod open_popover;
mod secret;
mod system_settings;
mod textbuffer;
mod window;

use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::prelude::*;
use gtk::{gio, glib};
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

use self::application::AardvarkApplication;
use self::config::*;
use self::connection_popover::ConnectionPopover;
use self::open_popover::OpenPopover;
use self::textbuffer::AardvarkTextBuffer;
use self::window::AardvarkWindow;

pub use self::config::APP_ID;

fn main() -> glib::ExitCode {
    setup_logging();

    // Construct base path within the app bundle for resources and data
    let mut base_bundle_path =
        std::env::current_exe().expect("Failed to get current executable path.");
    base_bundle_path.pop(); // -> Aardvark.app/Contents/MacOS/
    base_bundle_path.pop(); // -> Aardvark.app/Contents/

    // Set up gettext translations using path relative to bundle
    let mut locale_dir_path = base_bundle_path.clone();
    locale_dir_path.push("Resources");
    locale_dir_path.push("share");
    locale_dir_path.push("locale");

    if !locale_dir_path.exists() {
        // It's okay if it doesn't exist, gettext will just not find translations
        // but we might want to log this in a debug build.
        if cfg!(debug_assertions) {
            eprintln!(
                "Warning: Locale directory not found at expected bundle location: {:?}",
                locale_dir_path
            );
        }
    }

    bindtextdomain(
        GETTEXT_PACKAGE,
        locale_dir_path
            .to_str()
            .expect("Locale path is not valid UTF-8"),
    )
    .expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let mut resources_dir_path = base_bundle_path.clone();
    resources_dir_path.push("Resources");
    resources_dir_path.push("share");
    resources_dir_path.push("aardvark"); // Corresponds to the 'aardvark' subdirectory from PKGDATADIR structure

    let mut resources_file_path = resources_dir_path.clone();
    resources_file_path.push("resources.gresource");

    let mut ui_resources_file_path = resources_dir_path.clone();
    ui_resources_file_path.push("ui-resources.gresource");

    // Load resources using dynamically constructed paths
    if !resources_file_path.exists() {
        panic!(
            "GResource file 'resources.gresource' not found at expected bundle location: {:?}. Please check build script packaging.",
            resources_file_path
        );
    }
    let res = gio::Resource::load(&resources_file_path).unwrap_or_else(|err| {
        panic!(
            "Could not load gresource file from {:?}: {}",
            resources_file_path, err
        )
    });
    gio::resources_register(&res);

    if !ui_resources_file_path.exists() {
        panic!(
            "GResource file 'ui-resources.gresource' not found at expected bundle location: {:?}. Please check build script packaging.",
            ui_resources_file_path
        );
    }
    let ui_res = gio::Resource::load(&ui_resources_file_path).unwrap_or_else(|err| {
        panic!(
            "Could not load UI gresource file from {:?}: {}",
            ui_resources_file_path, err
        )
    });
    gio::resources_register(&ui_res);

    // Create a new GtkApplication. The application manages our main loop,
    // application windows, integration with the window manager/compositor, and
    // desktop features such as file opening and single-instance applications.
    let app = AardvarkApplication::new("org.p2panda.aardvark", &gio::ApplicationFlags::empty());

    info!("Aardvark ({})", APP_ID);
    info!("Version: {}", VERSION);
    info!("Datadir: {}", PKGDATADIR);

    // Run the application. This function will block until the application
    // exits. Upon return, we have our exit code to return to the shell. (This
    // is the code you see when you do `echo $?` after running a command in a
    // terminal.
    app.run()
}

fn setup_logging() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(EnvFilter::from_default_env())
        .try_init()
        .ok();
}
