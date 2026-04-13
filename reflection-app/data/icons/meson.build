install_data(
  '@0@.svg'.format(application_id),
  install_dir: iconsdir / 'hicolor' / 'scalable' / 'apps',
)

install_data(
  '@0@-symbolic.svg'.format(application_id),
  install_dir: iconsdir / 'hicolor' / 'symbolic' / 'apps',
)

if host_machine.system() == 'darwin'
  install_data(
    '@0@.icns'.format(application_id),
    install_dir : datadir)
endif
