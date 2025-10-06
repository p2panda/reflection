{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };
  outputs =
    { self, nixpkgs }:
    {
      packages.x86_64-linux.default = self.packages.x86_64-linux.reflection;
      packages.x86_64-linux.reflection = nixpkgs.legacyPackages.x86_64-linux.callPackage (
        {
          lib,
          stdenv,
          cargo,
          desktop-file-utils,
          meson,
          ninja,
          pkg-config,
          rustc,
          wrapGAppsHook4,
          rustPlatform,
          gtk4,
          gtksourceview5,
          libadwaita,
          libpanel,
          vte-gtk4,
          glib,
          openssl,
          blueprint-compiler,
          libspelling,
        }:
        stdenv.mkDerivation rec {
          pname = "reflection";
          version = "0.3rc0";

          src = ./.;

          cargoDeps = rustPlatform.fetchCargoVendor {
            inherit pname version src;
            hash = "sha256-cbrZJzKIVqrA49Tjd3xR9do04CV8Euhb3UjPs3PtqhI=";
          };

          PKG_CONFIG_PATH = "${openssl.dev}/lib/pkgconfig";

          nativeBuildInputs = [
            desktop-file-utils
            glib
            gtk4
            meson
            ninja
            pkg-config
            rustPlatform.cargoSetupHook
            cargo
            rustc
            wrapGAppsHook4
            blueprint-compiler
            libspelling
          ];

          buildInputs = [
            gtk4
            gtksourceview5
            libadwaita
            libpanel
            vte-gtk4
            libspelling
          ];

          meta = {
            description = "Collaborative, local-first GTK text editor";
            homepage = "https://github.com/p2panda/reflection";
            license = lib.licenses.gpl3Only;
            platforms = lib.platforms.linux;
            mainProgram = "reflection";
          };

        }
      ) { };
    };
}
