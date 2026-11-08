/* macos_bundle.rs
 *
 * Copyright 2026 The Reflection Developers
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

use std::env;
use std::ffi::{CString, c_char, c_void};
use std::path::{Path, PathBuf};

use tracing::warn;

// `RTLD_DEFAULT` from <dlfcn.h>
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

/// A self-contained app bundle created by `build-aux/build_macos.sh`.
///
/// The bundled libraries only know about the Homebrew prefix they were built for, so they have to
/// be pointed at the bundle at runtime.
pub struct Bundle {
    contents: PathBuf,
}

impl Bundle {
    /// Returns the bundle containing the running executable, wherever it was moved to.
    ///
    /// Returns `None` for installs that link against Homebrew, these have no `Frameworks`.
    pub fn locate() -> Option<Self> {
        let executable = env::current_exe().ok()?.canonicalize().ok()?;
        let contents = executable.parent()?.parent()?;

        (contents.file_name()? == "Contents" && contents.join("Frameworks").is_dir()).then(|| {
            Self {
                contents: contents.to_path_buf(),
            }
        })
    }

    pub fn locale_dir(&self) -> PathBuf {
        self.resources_dir().join("locale")
    }

    pub fn pkgdata_dir(&self) -> PathBuf {
        self.resources_dir().join("reflection")
    }

    /// Points the bundled libraries at the data and modules in the bundle.
    ///
    /// Has to be called before any other thread is started and before GTK is initialized.
    pub fn setup_environment(&self) {
        let resources = self.resources_dir();
        let plugins = self.contents.join("PlugIns");

        // SAFETY: Called at the very start of `main`, no other threads are running yet.
        unsafe {
            env::set_var("XDG_DATA_DIRS", &resources);
            env::set_var("GSETTINGS_SCHEMA_DIR", resources.join("glib-2.0/schemas"));
            env::set_var(
                "GDK_PIXBUF_MODULE_FILE",
                resources.join("gdk-pixbuf/loaders.cache"),
            );

            // Don't load modules from a Homebrew installation, they might not match our libraries.
            env::set_var("GIO_MODULE_DIR", plugins.join("gio"));
            env::set_var("GTK_EXE_PREFIX", &self.contents);
            env::set_var("GTK_DATA_PREFIX", &resources);
        }

        set_enchant_prefix(&plugins.join("enchant"));
    }

    fn resources_dir(&self) -> PathBuf {
        self.contents.join("Resources")
    }
}

/// Makes enchant look for its spell checking providers in `<prefix>/lib/enchant-2`.
///
/// Enchant has no environment variable for this. It is only linked through libspelling, so look up
/// the symbol instead of adding a build dependency.
fn set_enchant_prefix(prefix: &Path) {
    let Ok(prefix) = CString::new(prefix.as_os_str().as_encoded_bytes()) else {
        warn!("Bundle path contains a NUL byte, spell checking won't be available");
        return;
    };

    // SAFETY: The symbol name is a valid C string and `RTLD_DEFAULT` is a valid handle.
    let symbol = unsafe { dlsym(RTLD_DEFAULT, c"enchant_set_prefix_dir".as_ptr()) };
    if symbol.is_null() {
        warn!("Could not find enchant_set_prefix_dir, spell checking won't be available");
        return;
    }

    // SAFETY: `enchant_set_prefix_dir` has this signature and copies the string.
    unsafe {
        let set_prefix_dir: unsafe extern "C" fn(*const c_char) = std::mem::transmute(symbol);
        set_prefix_dir(prefix.as_ptr());
    }
}
