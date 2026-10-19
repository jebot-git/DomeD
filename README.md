# Dome

An experimental Wayland compositor with a classic stacking desktop and a shared
spatial desktop model, based on the supplied three-lane dome design. This is an
independent repository; it does not modify or depend on WiVRn-Pro.

**The native application is Rust.** Godot is restricted to experiments under
`prototypes/` and is not a production compositor dependency. Both the desktop WM
and the OpenXR frontend are Rust; a small C++ FFI adapter calls Qt6 painting/style APIs.

**Current milestone:** a nested Wayland compositor with Qt6 decorations, a classic
Mac-style global menu and a native Rust/OpenXR frontend displaying live Wayland
window textures, spherical dome surfaces, flat direct-view/pulled-out panels and
shared XR shell bars. Application identity/actions use XDG desktop entries; the
status area hosts StatusNotifier items and DBusMenu menus. WiVRn marker localization
and arbitrary application-internal global-menu import remain future work.

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
libxkbcommon-dev qt6-base-dev`. Qt6 Widgets 6.4 or newer supplies shell fonts,
menu styling and server-side window decorations. Then run from an existing Wayland or X11 desktop:

```sh
cargo run --locked --features nested --bin dome-compositor -- --command weston-terminal
```

The compositor opens a nested desktop window and prints its **own Wayland socket**.
It does not replace your running desktop or install a login session. Additional
clients can connect using that socket and the same `XDG_RUNTIME_DIR`:

```sh
WAYLAND_DISPLAY=wayland-N your-wayland-app
```

The native shell supports click-to-focus, Qt6 title-bar controls/dragging, client-initiated move/resize,
minimize/maximize/close, app launch (also Super+Space) and desktop switching/creation.
Its local web panel controls the same live window/workspace model; its geometry
preview still uses sample surface content. App launching requires `gio`, installed
`.desktop` files and apps with native Wayland support. No Xwayland is provided.
Close the nested compositor window to end the experiment. The initial protocol
handlers are adapted from Smithay's MIT-licensed Smallvil; see THIRD-PARTY.md.

## Native Rust XR frontend

Start `dome-compositor` and connect native Wayland clients first. With an active
OpenXR runtime (Monado or a connected WiVRn headset) and Vulkan available:

```sh
cargo run --locked --features xr --bin dome-xr -- --server http://127.0.0.1:49640
```

Use the compositor's printed port if it differs. `XR_RUNTIME_JSON` selects the
runtime for this process. No runtime registration or service settings are changed.
`--frames 240 --report /tmp/dome-xr.json --capture /tmp/dome-textures` runs a finite
session and writes diagnostics and **client texture** PNGs. Those PNGs are not eye
captures. The integration test separately captures the simulated headset output.

In **room dome** mode, attached windows use spherical OpenXR equirect2 layers,
curving in both axes, with ray-to-UV hits on the same sphere. In **direct view**,
window surfaces stay planar. `XR_KHR_composition_layer_equirect2` is required for
the dome; a runtime lacking it gets an explicit error instead of a flat substitute.

Grip a dome window and pull inward by 18 cm to detach it into a flat near panel.
There are exactly **two side-by-side slots** across the session. The slots anchor
in the room at the user's position/yaw when the first window is pulled, at a
configured depth of **1 m** (`pullout_distance_m`, range 0.6–1.6 m). They stay there
when the user moves their head. Push a gripped near panel outward by 18 cm to
return it to its stored dome position/scale. Releasing is required between these
transitions. A third pull leaves that window on the dome. One- and two-hand pulls
work; lateral grip movement adjusts an attached window along the dome. Scaling
near panels grows them outward from their central gap so they remain side by side.

The same Qt6 menu/tray/taskbar is rendered into two view-relative XR shell layers,
with configurable angular sizes and lens-safe insets. Menu and preview expansion
keeps the original top/bottom edges fixed. Hand pinches route into the native shell
commands, including application actions, task previews and status menus. Horizontal
bar widths are capped to the runtime field of view as well as the configured cap.

Real Wayland surfaces and shell images travel through a CPU bridge into Vulkan
swapchains. The compositor thread owns input and Wayland resource lifetimes. This
is an initial implementation, not a zero-copy renderer; window captures are capped
at about 20 Hz.

OpenXR `EXT_hand_tracking` supplies the 26 joints per hand when supported:

| Gesture | Input |
| --- | --- |
| Index/thumb pinch | Left mouse button |
| Middle/thumb pinch | Middle mouse button |
| Ring/thumb pinch | Right mouse button |
| Grip and move | Slide along the dome, or move a direct-view window |
| Pull / push a gripped dome panel | Pull into a near slot / return to dome |
| Grip the same window with both hands and change separation | Scale the panel |

White reticles show the hand targets and turn amber during a gesture. Pinches use
palm-normalized thresholds, debounce and hysteresis; a fist takes grip priority.
Both hands must reopen after tracking loss before another interaction. Drag capture
stays with its original window outside the window bounds. Tracking/focus loss,
large tracking jumps and an expired input sender release buttons and cancel grabs.
Return a window to its lane with **Shift+Super+R**.

The simulator has no tracked hands: joint/gesture logic is covered by Rust tests
and injected seat events by real Wayland-client tests. Physical hand ergonomics
still need headset testing. Controller bindings, complete application popup capture,
GPU buffer sharing and persistent marker localization remain follow-up work.
The optional Godot viewer remains a separate design prototype.

## XR routing, cursor ownership and desktop session

XR surfaces contain client content without compositor titlebars or borders; the
focused window expands **4 logical pixels on each edge**, filling four pixels
of the adjacent gap. A 1 mm depth bias keeps raised edges in front. Global menus
own window commands. Applications that draw controls into their own client buffers
retain those controls. Arbitrary application-internal global menus are not yet
imported.

Dome has two Wayland sockets, printed at startup. Desktop launches connect to the
normal socket and appear in both projections. Launches through the XR shell use
the XR socket and appear only in XR while its renderer is healthy. The desktop
taskbar filters those windows. Origin belongs to the Wayland connection, including
additional windows from that client. A single-instance application activated over
D-Bus can reuse its existing connection and therefore retains that connection's
origin.

OpenXR visibility and tracking status are reported separately from hand input.
Stopping/interruption promotes XR-only windows to the primary desktop without
relaunching them. Missing renderer heartbeats recover them within two seconds,
including after SIGKILL. They are unminimized, moved to desktop 1, clamped to the
primary work area and stacked. They stay on desktop when XR resumes. Focus loss
while the session remains visible suspends hand input without promoting windows.
The current nested backend has one primary output.

A physical mouse motion, button or wheel event takes exclusive XR cursor ownership,
releases hand buttons and cancels gestures. Movement starts at the last laser target.
Buttons retain mouse ownership while held; after release and **800 ms** without
mouse input, hand control resumes with neutral gesture rearming. The delay is
configurable. Wheel events target the hovered surface. Outside XR, mouse control
uses the normal desktop stacking path.

The system menu includes **Start XR**, **Stop XR**, **Appearance**, **Lock Screen**,
**Switch User**, **Sleep**, **Log Out of Dome**, **Restart** and **Shut Down**. Build
`dome-xr` alongside the desktop binary for Start XR. Lock delegates to the session's
screen locker; switching delegates to the display manager; sleep/power commands
use logind and its authorization policy. Unsupported commands are disabled. Lock,
switching and confirmed sleep/power requests also stop XR presentation. Logout
closes this nested Dome session, preserving the host login. This prototype does
not implement a standalone DRM/PAM login session or its own secure screen locker.

Appearance offers light/dark bars, accent, desktop background, bar opacity and
mouse-to-hand idle delay, persisted in `$XDG_CONFIG_HOME/dome/appearance.json`.
The global menu bar and taskbar use the same colours and opacity, in desktop and
XR. Global menu and status dropdowns share that palette.

XR window gaps are at most **8 Wayland logical content pixels**, horizontally
and between lanes, including **room-dome view**. Different client sizes and crowded rings use the smaller
adjacent angular pixel pitch to preserve the cap; the conversion is recalculated
with radius. Pulled-out panels use the same pixel-based gap. Focus expands the
visible surface four gap pixels per edge, using the same conservative pixel pitch
as packing across different client sizes and lane heights.
A **1-pixel inset border** is drawn by the native XR frontend, configurable from
0 to **2 pixels**; it never adds to the gap or the hit area. The layout laboratory
exposes `window_gap_px` (0–8) and `window_border_px` (0–2). Older profiles' angular
`window_gap_deg` is retained for compatibility; the pixel setting controls spacing.
Manually moved spatial panels retain their explicit placement.

Window surfaces follow the configured sphere, with curvature `1 / radius`.
Enlarging the dome preserves reference card height while shrinking angular sizes.
Direct-view windows remain flat. Focus changes the same geometry used by lasers
and mouse hit testing; its visual outset is not saved into gesture transforms.

## Application menus, global bar and taskbar

The application catalog reads standard XDG data directories and recursive desktop
file IDs, with user overrides/hidden tombstones, locale-aware names, icons,
categories, keywords, visibility rules, TryExec and Desktop Actions. GIO performs
launching and field-code expansion. The active application's global menu derives
its name and actions from its desktop entry, alongside compositor window/edit
commands. It does not invent an application's internal menu tree.

The global bar follows the classic Apple arrangement: system menu, bold active
application, File/Edit/View/Window/Help, and right-aligned monochrome status extras
and clock. StatusNotifierItem icons and DBusMenu actions run through a Rust worker;
Dome uses an existing watcher when present and creates one only when absent.
Application command menus are separate from XDG desktop entries: importing
arbitrary toolkit-exported application menus is still future work.

The taskbar follows Plasma-style behavior: group by application, activate/minimize
single tasks, choose windows through live thumbnail previews, wheel-cycle a group,
middle-click for a new instance, and right-click for desktop actions, pin/unpin and
Close All. Pins use desktop-file IDs and persist in
`$XDG_CONFIG_HOME/dome/taskbar.json`. Launcher search remains at the left and the
virtual desktop manager at the right.

## Keyboard shortcuts

Super is the Windows/Meta key. Application Ctrl shortcuts continue to work.

| Shortcut | Action |
| --- | --- |
| Super+Space | Search and launch apps |
| Alt+Tab / Shift+Alt+Tab | Next / previous window |
| Super+W or Alt+F4 | Close window |
| Super+Q | Close windows of active application |
| Super+M / Super+Enter | Minimize / toggle maximize |
| Super+1…8 / Shift+Super+1…8 | Switch desktop / move window to desktop |
| Ctrl+Super+Left/Right | Previous / next desktop |
| Ctrl+Super+Up/Down | Move window between lanes |
| Shift+Super+N | Create desktop |
| Super+Plus/Minus | Increase / decrease XR window scale |
| Shift+Super+R | Reset XR placement and scale |
| Shift+Super+B | Slide both XR bars out / back in |
| Alt+Super+T | Slide upper XR menu/tray bar out / back in |
| Alt+Super+B | Slide XR taskbar out / back in |
| Shift+Super+D | Fade the entire XR desktop out / back in |
| Super+C/V/A | Copy / paste / select all using client Ctrl bindings |
| F10, arrows, Enter, Escape | Navigate classic global menus |

XR visibility controls are also in **View**. Bar slides and desktop fades take
250 ms and reverse continuously if toggled mid-animation. The upper bar slides
past the top view edge; the taskbar slides past the bottom. Hiding the XR desktop
fades all window and shell layers, preserving each bar's visibility preference.
These controls work in dome and direct-view modes and remain available from
launchers and shell settings. Hidden/transitioning surfaces reject XR pointer
input; hiding the desktop releases gestures and blocks new typing until restored.
Desktop window placement and monitor bars are unaffected. This is a presentation
toggle: the XR session stays alive and does not trigger contingency recovery.

The global bar supplies compositor commands in App, File, Edit, View, Window and
Help menus. It does not yet import arbitrary application-exported menu trees.

## Run the XR prototype in a simulated headset

The optional `prototypes/godot-xr/` is a Godot 4.7 OpenXR prototype of the same Rust meshes.
It submits real stereo frames and follows runtime head poses. Window content is
still sample content; this does not implement Wayland buffer transport to XR.
It is a development viewer, not a Godot dependency for the compositor itself.

With Monado, a Vulkan-capable desktop and Godot 4.7 available:

```sh
cargo build --locked --bin dome-preview
python3 prototypes/run-godot-xr.py --godot /path/to/godot
# Leave a simulated headset and viewer running:
python3 prototypes/run-godot-xr.py --godot /path/to/godot --live
# Or show the layout service already running in the browser/native compositor:
python3 prototypes/run-godot-xr.py --live --scene-url http://127.0.0.1:49640
```

The launcher starts an isolated Monado service with `SIMULATED_ENABLE=1`, its own
runtime socket directory, an explicit per-process OpenXR manifest and a private
sample scene by default. It leaves existing WiVRn and Monado user services alone.
Closing the viewer/interrupting the launcher stops only its children. The finite
run checks native stereo, session focus, distinct runtime eye poses, shared mesh
loading and both projections. It reads back both rendered eye buffers and checks
that they differ and contain scene pixels. Reports and images are written to
`artifacts/simulated-xr/`; the interactive run uses `artifacts/simulated-xr-live/`.

The installed Godot 4.7.2 build reports an early `xrEndSession`, a spatial-extension
signal disconnect and an interaction-profile RID at shutdown. These are retained
explicitly in `engine_shutdown_diagnostics` rather than hidden; other errors fail
the test. No controller interaction, persistent anchor or streaming claim follows
from the rendering checks.

A local WiVRn server is not itself a simulated headset. The launcher records its
`HeadsetConnected`/`SessionRunning` properties without pairing or disconnecting it.
`--runtime wivrn` selects its installed manifest and requires a connected headset
client; with no client it fails clearly. Monado's simulated HMD does not establish
a WiVRn network stream. The local WiVRn-Pro client source also enforces a Wi-Fi
route and rejects loopback; this project does not change that policy.

## Validate

```sh
cargo test --locked
cargo check --locked --features nested
cargo build --locked --features nested
python3 scripts/test-service.py
# With Weston and Mesa Wayland examples installed:
python3 scripts/test-native.py
# Full desktop input and framebuffer tests on an isolated display (requires Xvfb):
uv run --no-project --with PySide6 --with python-xlib --with pillow --with dbus-next python scripts/test-desktop.py
# With dome-preview running, optional offscreen visual/interaction check:
uv run --no-project --with PySide6 python scripts/preview-smoke.py
```

The scene tests cover frame-mode isolation, lens margins and bar geometry,
nonoverlap, resizing, swaps, workspace lifetimes, maximization, mesh generation
and ray picking. The native backend has also been exercised under isolated
headless Weston/Mesa with a real EGL Wayland client through map, maximize,
minimize, restore, workspace changes and close. This is not headset validation.

The full desktop test adds real pointer/keyboard input and framebuffer assertions:
stacking through repaints, clipboard transfer, hover-scroll focus, empty-space
defocus, client move/resize requests using real input serials, output resizing, native menu/taskbar
actions, desktop management, launcher shortcut/input ownership and client crash
cleanup. Results and native output screenshots are saved to `artifacts/desktop/`.
See [desktop validation](docs/DESKTOP-VALIDATION.md).

The [original design draft](docs/design-draft.png) is retained with the project.
See [docs/DESIGN.md](docs/DESIGN.md) for the coordinate-frame contract and roadmap.
