project('reflection', 'rust',
          version: '0.2',
    meson_version: '>= 1.0.0',
  default_options: [ 'warning_level=2', 'werror=false', ],
)

i18n = import('i18n')
gnome = import('gnome')

application_id = 'cx.modal.Reflection'

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
localedir = prefix / get_option('localedir')

datadir = prefix / get_option('datadir')
pkgdatadir = datadir / meson.project_name()
iconsdir = datadir / 'icons'
podir = meson.project_source_root() / 'po'

subdir('reflection-app')

gnome.post_install(
     glib_compile_schemas: true,
    gtk_update_icon_cache: true,
  update_desktop_database: true,
)
