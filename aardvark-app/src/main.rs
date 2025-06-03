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
    load_resources();

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

fn load_resources() {
    // Used for macOS app bundle
    let mut base_bundle_path =
        std::env::current_exe().expect("Failed to get current executable path.");
    base_bundle_path.pop(); // -> Aardvark.app/Contents/MacOS/
    base_bundle_path.pop(); // -> Aardvark.app/Contents/

    let local_dir_path = if cfg!(target_os = "macos") {
        base_bundle_path.join(LOCALEDIR)
    } else {
        PathBuf::from(LOCALEDIR)
    };

    let resources_dir_path = if cfg!(target_os = "macos") {
        base_bundle_path.join(PKGDATADIR)
    } else {
        PathBuf::from(PKGDATADIR)
    };

    bindtextdomain(
        GETTEXT_PACKAGE,
        local_dir_path
            .to_str()
            .expect("Locale path is not valid UTF-8"),
    )
    .expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let res = gio::Resource::load(resources_dir_path.join("resources.gresource"))
        .expect("Could not load gresource file");
    gio::resources_register(&res);

    let ui_res = gio::Resource::load(resources_dir_path.join("ui-resources.gresource"))
        .expect("Could not load UI gresource file");
    gio::resources_register(&ui_res);
}
