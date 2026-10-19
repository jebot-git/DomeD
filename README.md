# Dome

An experimental Wayland compositor with a classic stacking desktop and a shared
spatial desktop model, based on the supplied three-lane dome design. This is an
independent repository; it does not modify or depend on WiVRn-Pro.

**First milestone:** a runnable nested Wayland compositor and an interactive
geometry laboratory. OpenXR presentation, WiVRn marker localization, application
menu export and system tray services are the next milestone, not implemented yet.

![Dome layout laboratory](docs/preview.png)

## Try the layout laboratory

With a Rust toolchain installed:

```sh
cargo run --locked --bin dome-preview
```

Open **http://127.0.0.1:49640**. No headset, GPU development libraries, JavaScript
packages or network assets are required. The preview starts with sample windows.

* Switch **XR carousel / Desktop** to see the same window identities in both views.
* Choose **Room carousel** for an anchored, full-circle layout independent of the
  view trays, or **In view** to fit lanes between the upper tray and taskbar.
* Set bar widths, heights, independent lens-safe insets, FOV, room centre and radius.
* Drag empty space to simulate looking around. **Unfold 360°** exposes the whole ring.
* Select windows to minimize/maximize, recenter, move between lanes/desktops or swap.
* **Launch** at left searches apps; layout mode adds a sample window instead of
  launching. **Desktops** at right switches, creates, renames and removes workspaces.
* Hover task groups to expand their window list. Wheel over sample surfaces to
  demonstrate scroll targeting while leaving keyboard focus unchanged.

Geometry edits are session-local. **Export geometry JSON** saves a reusable
profile. To start with that profile:

```sh
DOME_CONFIG=/path/to/dome.json cargo run --locked --bin dome-preview
```

`DOME_PREVIEW_PORT` overrides the default loopback port. Use the displayed numeric
loopback URL; the development API intentionally rejects foreign origins/hosts.

## Run real Wayland clients

Install a Rust toolchain, C linker, `pkg-config`, Wayland/EGL/GLES and xkbcommon
development packages. On Ubuntu/Debian the development packages are typically
`build-essential pkg-config libwayland-dev libegl1-mesa-dev libgles2-mesa-dev
libxkbcommon-dev`. Then run from an existing Wayland or X11 desktop:

```sh
cargo run --locked --features nested --bin dome-compositor -- --command weston-terminal
```

The compositor opens a nested desktop window and prints its **own Wayland socket**.
It does not replace your running desktop or install a login session. Additional
clients can connect using that socket and the same `XDG_RUNTIME_DIR`:

```sh
WAYLAND_DISPLAY=wayland-N your-wayland-app
```

The native shell supports click-to-focus, client-initiated move/resize,
minimize/maximize/close, app launch (also Super+Space) and desktop switching/creation.
Its local web panel controls the same live window/workspace model; its geometry
preview still uses sample surface content. App launching requires `gio`, installed
`.desktop` files and apps with native Wayland support. No Xwayland is provided.
Close the nested compositor window to end the experiment. The initial protocol
handlers are adapted from Smithay's MIT-licensed Smallvil; see THIRD-PARTY.md.

## Validate

```sh
cargo test --locked
cargo check --locked --features nested
cargo build --locked --features nested
python3 scripts/test-service.py
# With Weston and Mesa Wayland examples installed:
python3 scripts/test-native.py
# With dome-preview running, optional offscreen visual/interaction check:
uv run --no-project --with PySide6 python scripts/preview-smoke.py
```

The scene tests cover frame-mode isolation, lens margins and bar geometry,
nonoverlap, resizing, swaps, workspace lifetimes, maximization, mesh generation
and ray picking. The native backend has also been exercised under isolated
headless Weston/Mesa with a real EGL Wayland client through map, maximize,
minimize, restore, workspace changes and close. This is not headset validation.

The [original design draft](docs/design-draft.png) is retained with the project.
See [docs/DESIGN.md](docs/DESIGN.md) for the coordinate-frame contract and roadmap.
