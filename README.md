<p align="center"><img src="reflection-app/data/icons/cx.modal.reflection.svg"></p>
<h1 align="center">Reflection</h1>
<p align="center">Collaboratively take meeting notes, even when there's no internet</p>

![Reflection app screenshot](reflection-app/data/screenshots/screenshot.png)

> [!CAUTION]
> The project is under active development and not considered stable yet. It
> probably won't eat your data, but no guarantees :)

## Development

### Getting Started

The [GNOME Builder IDE](https://builder.readthedocs.io/) is required to build
and run the project. It can be installed with flatpak.

1. [Install flatpak](https://flatpak.org/setup/) for your distribution.
2. Install [Builder](https://flathub.org/apps/org.gnome.Builder) for GNOME:
    `flatpak install flathub org.gnome.Builder`
3. Clone the reflection repo:
    `git clone git@github.com:p2panda/reflection.git && cd reflection`
4. Open the Builder application and navigate to the reflection repo.
   - You may be prompted to install or update the SDK in Builder.
5. Run the project with `Shift+Ctrl+Space` or click the ► icon (top-middle of
   the Builder application).

### Multiple Instances

If you need multiple instances of the app on the same computer for testing,
open a "runtime terminal" in Builder and then run as many instances as you
need, like so:

```bash
# Launch three independent reflection instances
reflection & dbus-run-session reflection & dbus-run-session reflection
```

Make sure you've compiled the `reflection` binary already once (step 5. in
"Getting Started"), to be able to execute the program in the "runtime
terminal".

### Diagnostics

Set the `RUST_LOG` environment variable to your verbosity setting and filter to
enable log-based diagnostics with [tracing](https://docs.rs/tracing). Example:
`RUST_LOG=DEBUG` or `RUST_LOG=p2panda_net=INFO` etc.

Use the "runtime terminal" in Builder and set the environment variable like
that:

```bash
# Launch reflection with logging enabled, set to verbosity level "warn"
RUST_LOG=WARN reflection

# Launch two instances with logging. We can set the environment variable for
# the current runtime, all instances will have logging enabled
RUST_LOG=p2panda_net=DEBUG,iroh=WARN
reflection & dbus-run-session reflection
```

Make sure you've compiled the `reflection` binary already once (step 5. in
"Getting Started"), to be able to execute the program in the "runtime
terminal".

## License

[GNU General Public License v3.0](COPYING)

## Supported By

Thanks to [NLNet](https://nlnet.nl) (via [NGI0
ENTRUST](https://nlnet.nl/project/P2Panda-groups/)) under grant agreement No
101069594, the [Prototype Fund](https://www.prototypefund.de/), and the
[Federal Ministry of Research, Technology and Space](https://www.bmbf.de/EN/)
for funding this project.

![Nlnet Logo](assets/logo-nlnet.jpg)
![Ministry Logo](assets/logo-bmftr.jpg)
![Prototype Fund Logo](assets/logo-prototypefund.jpg)
