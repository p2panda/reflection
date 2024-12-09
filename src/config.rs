use std::path::PathBuf;
use std::env;

pub static VERSION: &str = "0.1.0";
pub static GETTEXT_PACKAGE: &str = "aardvark";
pub static LOCALEDIR: &str = "/app/share/locale";

#[cfg(target_os = "macos")]
pub fn get_pkgdatadir() -> PathBuf {
    let exe_path = env::current_exe().expect("Failed to get current executable path");

    // Navigate to the 'Resources/share/aardvark' directory relative to the executable
    let pkgdatadir = exe_path
        .parent()       // Goes up to 'Contents/MacOS'
        .and_then(|p| p.parent()) // Goes up to 'Contents'
        .map(|p| p.join("Resources/share/aardvark"))
        .expect("Failed to compute PKGDATADIR");

    pkgdatadir
}

#[cfg(not(target_os = "macos"))]
pub fn get_pkgdatadir() -> PathBuf {
    PathBuf::from("/app/share/aardvark")
}
