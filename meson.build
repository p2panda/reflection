project('reflection', 'rust',
          version: '0.1',
    meson_version: '>= 1.0.0',
  default_options: [ 'warning_level=2', 'werror=false', ],
)

i18n = import('i18n')
gnome = import('gnome')

application_id = 'cx.modal.Reflection'

pkgdatadir = get_option('prefix') / get_option('datadir') / meson.project_name()
iconsdir = get_option('datadir') / 'icons'

subdir('reflection-app/data')
subdir('reflection-app/src')
subdir('reflection-app/po')

gnome.post_install(
     glib_compile_schemas: true,
    gtk_update_icon_cache: true,
  update_desktop_database: true,
)
