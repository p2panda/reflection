pub static VERSION: &str = "0.1.0";
pub static GETTEXT_PACKAGE: &str = "aardvark";
pub static LOCALEDIR: &str = "/app/share/locale";
#[cfg(target_os = "macos")]
pub static PKGDATADIR: &str = "../Resources/share/aardvark";

#[cfg(not(target_os = "macos"))]
pub static PKGDATADIR: &str = "/app/share/aardvark";
