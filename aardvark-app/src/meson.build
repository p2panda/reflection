gnome = import('gnome')

gnome.compile_resources('ui-resources',
  'ui-resources.gresource.xml',
  gresource_bundle: true,
  install: true,
  install_dir: pkgdatadir,
)

# Detect if we're building for a macOS app bundle
is_macos_bundle = get_option('macos_bundle')

conf = configuration_data()
conf.set_quoted('APP_ID', application_id)
conf.set_quoted('VERSION', meson.project_version())
conf.set_quoted('GETTEXT_PACKAGE', 'aardvark')

# Set paths based on whether we're building for macOS app bundle or not
if is_macos_bundle
  # For macOS app bundles, use relative paths that will be resolved at runtime
  conf.set_quoted('LOCALEDIR', 'Resources/share/locale')
  conf.set_quoted('PKGDATADIR', 'Resources/share/aardvark')
else
  # For Flatpak and other installations, use absolute paths
  conf.set_quoted('LOCALEDIR', get_option('prefix') / get_option('localedir'))
  conf.set_quoted('PKGDATADIR', pkgdatadir)
endif

configure_file(
    input: 'config.rs.in',
    output: 'config.rs',
    configuration: conf
)

# Copy the config.rs output to the source directory.
run_command(
  'cp',
  meson.project_build_root() / 'aardvark-app/src' / 'config.rs',
  meson.project_source_root() / 'aardvark-app/src' / 'config.rs',
  check: true
)

cargo_bin  = find_program('cargo')
cargo_opt  = [ '--manifest-path', meson.project_source_root() / 'Cargo.toml' ]
cargo_opt += [ '--target-dir', meson.project_build_root()  / 'aardvark-app/src' ]
cargo_env  = [ 'CARGO_HOME=' + meson.project_build_root()  / 'cargo-home' ]

if get_option('buildtype') == 'release'
  cargo_opt += [ '--release' ]
  rust_target = 'release'
else
  rust_target = 'debug'
endif

cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: meson.project_name(),
  console: true,
  install: true,
  install_dir: get_option('bindir'),
  command: [
    'env', cargo_env,
    cargo_bin, 'build',
    cargo_opt, '&&', 'cp', 'aardvark-app/src' / rust_target / meson.project_name(), '@OUTPUT@',
  ]
)
