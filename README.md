# Aardvark (working title)

MVP collaborative, local-first GTK text editor :)

## Development

### Getting Started

The [GNOME Builder IDE](https://builder.readthedocs.io/) is required to build
and run the project. It can be installed with flatpak.

1. [Install flatpak](https://flatpak.org/setup/) for your distribution.
2. Install [Builder](https://flathub.org/apps/org.gnome.Builder) for GNOME:
    `flatpak install flathub org.gnome.Builder`
3. Clone the aardvark repo:
    `git clone git@github.com:p2panda/aardvark.git && cd aardvark`
4. Open the Builder application and navigate to the aardvark repo.
   - You may be prompted to install or update the SDK in Builder.
5. Run the project with `Shift+Ctrl+Space` or click the ► icon (top-middle of
   the Builder appication).

### Multiple instances

Run builder in a separate dbus session if you need multiple instances to test
the application: `dbus-run-session org.gnome.Builder`.

### Diagnostics

Set the `RUST_LOG` environment variable to your verbosity setting and filter to
enable log-based diagnostics with [tracing](https://docs.rs/tracing). Example:
`RUST_LOG=debug` or `RUST_LOG=p2panda_net=INFO` etc.

## Todo

> This is a list of ideas which came up during our hacky GTK + Rust + Automerge + p2panda hackfest (December 24, Berlin) trying to get a working POC together.

- [ ] UI: Creating and joining a new document flow
- [ ] UI: Multi-cursor support
- [ ] Automerge: Keep message size below 4096 bytes to not exceed gossip broadcast limit
    - Come up with own way to send patches over the wire as `save_incremental` seems to be too naive
    - Detect large Copy & Paste insertions above limit and insert it in "batches"
- [ ] p2panda: Look into max. reorder attempt bug
- [ ] p2panda: Re-attempt sync after being offline bug
- [ ] Frequently do full-state "snapshots" with automerge and prune p2panda log
    - For example, do it every x minutes or after someone pressed "Save"?
