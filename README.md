# Aardvark

MVP local-first text editor :)

## Getting Started

The [GNOME Builder IDE](https://builder.readthedocs.io/) is
required to build and run the project. It can be installed with flatpak.

1. [Install flatpak](https://flatpak.org/setup/) for your distribution.

2. Install [Builder](https://flathub.org/apps/org.gnome.Builder) for GNOME:

`flatpak install flathub org.gnome.Builder`

3. Clone the aardvark repo:

`git clone git@github.com:p2panda/aardvark.git && cd aardvark`

4. Open the Builder application and navigate to the aardvark repo.
   - You may be prompted to install or update the SDK in Builder.

5. Run the project with `Shift+Ctrl+Space` or click the ► icon (top-middle
   of the Builder appication).

## Todo

> This is a list of ideas which came up during our hacky GTK + Rust + Automerge + p2panda hackfest (December 24, Berlin) trying to get a working POC together for an offline-first text editor.

- [ ] UI: Creating and joining a new document flows
- [ ] Automerge root key needs to be safely transmitted
     - Currently the first peer who writes a key-stroke determines the (random?) root key. We want this to happen in a way where other peers learn about this root key before they contribute to the document.
- [ ] Why do we get empty strings from the `update_test` GTK callback?
    - This causes some weird infinite loop behaviour we could only hack around by ignoring empty strings
- [ ] Intercept key-strokes before sending it to textfield
- [ ] p2panda: Look into max. reorder attempt bug
- [ ] p2panda: Re-attempty sync after being offline bug
- [ ] Keep the cursor where it was when receiving remote updates
- [ ] Understand better how text CRDTs in automerge work
- [ ] UI: Multi-cursor support?
- [ ] Frequently do full-state "snapshots" with automerge and prune p2panda log
    - For example, do it every x minutes or after someone pressed "Save"?
