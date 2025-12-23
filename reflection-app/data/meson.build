# Desktop file
desktop_conf = configuration_data()
desktop_conf.set('icon', application_id)
desktop_file = i18n.merge_file(
    type: 'desktop',
    input: configure_file(
        input: '@0@.desktop.in.in'.format(application_id),
        output: '@BASENAME@',
        configuration: desktop_conf,
    ),
    output: '@0@.desktop'.format(application_id),
    po_dir: podir,
    install: true,
    install_dir: datadir / 'applications',
)

desktop_utils = find_program('desktop-file-validate', required: false)
if desktop_utils.found()
  test('Validate desktop file', desktop_utils, args: [desktop_file])
endif

appstream_file = i18n.merge_file(
     input: 'cx.modal.Reflection.metainfo.xml.in',
     output: '@0@.metainfo.xml'.format(application_id),
     po_dir: podir,
     install: true,
     install_dir: datadir / 'metainfo',
)

appstreamcli = find_program('appstreamcli', required: false, disabler: true)
test('Validate appstream file', appstreamcli,
     args: ['validate', '--no-net', '--explain', appstream_file])

install_data('cx.modal.Reflection.gschema.xml',
  install_dir: get_option('datadir') / 'glib-2.0' / 'schemas'
)

compile_schemas = find_program('glib-compile-schemas', required: false, disabler: true)
test('Validate schema file',
     compile_schemas,
     args: ['--strict', '--dry-run', meson.current_source_dir()])


# D-Bus service file
service_conf = configuration_data()
service_conf.set('application_id', application_id)
service_conf.set('bindir', bindir)
configure_file(
    input: 'cx.modal.Reflection.service.in',
    output: '@0@.service'.format(application_id),
    configuration: service_conf,
    install: true,
    install_dir: datadir / 'dbus-1/services',
)

subdir('icons')
subdir('resources')