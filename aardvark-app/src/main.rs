/* main.rs
 *
 * Copyright 2024 Tobias
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
mod config;
mod document;
mod textbuffer;
mod window;

use std::path::PathBuf;
use self::application::AardvarkApplication;
use self::textbuffer::AardvarkTextBuffer;
use self::window::AardvarkWindow;


use config::{GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::prelude::*;
use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    // Set up gettext translations
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    // Load resources
    let resources = gio::Resource::load(get_pkgdatadir().join("aardvark.gresource"))
        .expect("Could not load resources");
    gio::resources_register(&resources);

    // Create a new GtkApplication. The application manages our main loop,
    // application windows, integration with the window manager/compositor, and
    // desktop features such as file opening and single-instance applications.
    let app = AardvarkApplication::new("org.p2panda.aardvark", &gio::ApplicationFlags::empty());

    // Run the application. This function will block until the application
    // exits. Upon return, we have our exit code to return to the shell. (This
    // is the code you see when you do `echo $?` after running a command in a
    // terminal.
    app.run()
}

fn get_pkgdatadir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let exe_path = std::env::current_exe().expect("Failed to get current executable path");
        // Navigate to the 'Resources/share/aardvark' directory relative to the executable
        exe_path
            .parent()       // Goes up to 'Contents/MacOS'
            .and_then(|p| p.parent()) // Goes up to 'Contents'
            .map(|p| p.join("Resources/share/aardvark"))
            .expect("Failed to compute PKGDATADIR")
    }

    #[cfg(not(target_os = "macos"))]
    {
        PathBuf::from(PKGDATADIR)
    }
}
