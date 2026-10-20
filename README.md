# DomeD

An experimental Wayland compositor with a classic stacking desktop and a shared
spatial desktop model, based on the supplied three-lane dome design. This is an
independent repository; it does not modify or depend on WiVRn-Pro.

**The native application is Rust.** Godot is restricted to experiments under
`prototypes/` and is not a production compositor dependency. Both the desktop WM
and the OpenXR frontend are Rust; a small C++ FFI adapter calls Qt6 painting/style APIs.

**Current milestone:** a standalone DRM/KMS Wayland compositor with Qt6 decorations, a classic
Mac-style global menu and a native Rust/OpenXR frontend displaying live Wayland
window textures, open cylindrical dome surfaces, curved pulled-out panels and
shared XR shell bars. Application identity/actions use XDG desktop entries; the
status area hosts StatusNotifier items and DBusMenu menus. Focused applications
can export their real menu bars through KDE AppMenu and DBusMenu. WiVRn marker
localization remains future work.

![Dome layout laboratory](docs/preview.png)

## Try the layout laboratory

With a Rust toolchain installed:

```sh
cargo run --locked --bin dome-preview
```

Open **http://127.0.0.1:49640**. No headset, GPU development libraries, JavaScript
packages or network assets are required. The preview starts with sample windows.

* Switch **XR carousel / Desktop** to see the same window identities in both views.
* Use the room dome for an anchored cylindrical layout. Legacy **In view**
  profiles migrate to the dome; Ghostty XR owns its independent direct-view overlay.
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

Ghostty XR is DomeD's default terminal and retains its independent native XR
overlay, input focus and positioning controls. In the XR input realm, **Super+T**
opens it or creates and selects a new tab in its existing overlay. In the desktop
realm, including while XR is suspended, it opens an independent Wayland terminal
without changing the XR terminal. **F12** hides Ghostty and releases keyboard input back to
Dome. The overlay uses placement 20, above Dome's placement 10.

```sh
./scripts/build-local.sh
./scripts/run-local.sh
```

`DOME_TERMINAL` selects the executable (a path without arguments). The local
runner prefers `/usr/local/bin/ghostty-xr-launch`, with the sibling build as a
fallback. `DOME_TERMINAL_MODE=wayland` explicitly selects the separate Wayland
terminal mode in the XR realm as well. Desktop launches always use Wayland.
`dome-compositor --terminal` requests an initial terminal; launching without
arguments leaves an empty desktop. `--command PROGRAM ARGS...` still launches a
custom client. The local runner forwards explicit arguments unchanged.

The local build includes `dome-compositor`, `dome-preview` and `dome-xr` and runs
the Rust tests. On this Nobara/Fedora workstation the missing Qt6 development
headers are extracted under `.local-deps/root`, and `build-local.sh` uses those
headers with the installed `/usr/lib64` Qt runtime. No system installation is
needed. To recreate this optional local dependency setup:

```sh
./scripts/prepare-local-deps.sh
```

For an isolated integration check, with python-xlib and Pillow available:

```sh
python3 scripts/test-terminal.py
```

The test uses a private Xvfb display and D-Bus session, types through DomeD's real
Wayland seat into the shell, opens a second terminal with Super+T, captures a
client texture through the XR bridge and checks window management and cleanup.
Logs and `terminal.png` are saved in `artifacts/terminal/`. This verifies the XR
texture transport without a headset; headset appearance still needs visual testing.

Install a Rust toolchain, C linker, `pkg-config`, Wayland/EGL/GLES and xkbcommon
development packages. On Ubuntu/Debian the development packages are typically
`build-essential pkg-config libwayland-dev libegl1-mesa-dev libgles2-mesa-dev
libxkbcommon-dev qt6-base-dev libfontconfig1-dev libfreetype-dev libglib2.0-dev`. Qt6 Widgets 6.4 or newer supplies shell fonts,
menu styling and server-side window decorations. Then run from an existing Wayland or X11 desktop:

```sh
cargo run --locked --features nested --bin dome-compositor -- --terminal
```

The compositor opens a nested desktop window and prints its **own Wayland socket**.
This invocation runs inside your current desktop. To use it from the login
screen, install the session described below. Additional clients can connect using that socket and the same `XDG_RUNTIME_DIR`:

```sh
WAYLAND_DISPLAY=wayland-N your-wayland-app
```

The native shell supports click-to-focus, Qt6 title-bar controls/dragging, client-initiated move/resize,
minimize/maximize/close, app launch (also Super+Shift+Space) and desktop switching/creation.
Its local web panel controls the same live window/workspace model; its geometry
preview still uses sample surface content. App launching requires `gio`, installed
`.desktop` files and apps with native Wayland support. No Xwayland is provided.
Close the nested compositor window to end the experiment. The initial protocol
handlers are adapted from Smithay's MIT-licensed Smallvil; see THIRD-PARTY.md.

## System installation and login session

```sh
./scripts/build-local.sh --release
./scripts/stage-install.sh
pkexec /usr/bin/bash "$PWD/scripts/install-system.sh"
```

The installer puts release binaries in `/usr/local/bin`, session helpers in
`/usr/local/libexec`, documentation in `/usr/local/share/doc/domed`, and the
**DomeD** login entry in `/usr/share/wayland-sessions/dome.desktop`. It leaves
Plasma and the current login running. Log out, select **DomeD** in SDDM's session
menu, then log in. Replaced files are backed up under `/var/backups/domed`;
`/usr/local/share/domed/install.sha256` records installed file checksums.

The login session launches **DomeD directly**. Its native backend owns DRM/KMS
scanout, GBM/EGL rendering, a libseat/logind seat and libinput keyboard/pointer
devices. No KWin, Weston or other host compositor is started. **Log Out** stops
Dome and its terminal/XR children and returns to the login screen.

Run `dome-compositor --backend drm` from an unused TTY, or select DomeD at login.
Use `--backend nested` for a development window in an existing desktop. Without
an explicit backend, a host DISPLAY/WAYLAND_DISPLAY selects nested mode; otherwise
DRM is selected. `--check-backend` reports compiled support without opening a seat.

The native backend enumerates the seat's GPUs and drives every connected output,
with independent CRTC assignment, scanout buffers, damage and page-flip scheduling.
Displays form a horizontal extended desktop by default. Each monitor has its own
bars; windows, cursors and popups use global desktop coordinates, and maximization
reserves the containing monitor's bars. Unplugging a monitor recovers stranded
windows; reconnecting it restores scanout without restarting the session. GPUs can
also be added or removed while other outputs remain active. Each GPU has its own
GBM/EGL renderer; advertised DMA-BUF formats are supported by every initial GPU.

Open **Displays & XR Configuration** (**Super+,**) to select each connector's
resolution/refresh, enable it, choose a primary display, or position it explicitly
(including negative coordinates). Automatic arrangement restores horizontal layout.
Hold **Shift** while dragging either a client titlebar or a DomeD titlebar to tile
a window into halves at monitor edges or quarters at corners. A translucent preview
shows the destination; drop while holding Shift to apply it, or release Shift to
cancel tiling. Tiling reserves the target monitor's bars and decoration borders.
New windows open on the primary display. Connector preferences are saved with the
XR configuration in `$XDG_CONFIG_HOME/dome/xr.json`; PCI addresses distinguish ports
on different GPUs. `DOME_DRM_DEVICE=/dev/dri/cardN` restricts diagnostics to one GPU;
`DOME_DRM_CONNECTOR=DP-1` restricts connectors by their kernel connector name.
With no connected monitor, a virtual 1280×800 desktop keeps XR clients rendering.

XR configuration uses a fixed 1024×768 canvas, independent of monitor resolution.
Expanded configuration follows the selected projection, anchor, pane distance and
vertical view field rather than stretching into a bar. Rows retain readable height;
wheel/hand scrolling, the scroll buttons, arrows, Page Up/Down and Home/End navigate
all options. The selected row stays visible and hit testing follows the viewport.

**Ctrl+Alt+F1…F12** switches VTs through libseat. Seat deactivation stops scanout and
input, balances held buttons/keys, stops XR input and hides the terminal overlay.
Reactivation restores scanout and client focus. Native Wayland clients can use
DMA-BUF buffers; client cursor surfaces and a fallback software cursor are drawn
by Dome. The nested and native backends share shell, popup and XR rendering.

The session requests XR and the native Ghostty overlay at startup. If the headset
or runtime is unavailable, the desktop remains usable; use **Start XR** when it
becomes available. `DOME_START_XR=0` disables automatic XR startup and
`DOME_SESSION_TERMINAL=0` starts without a terminal. The saved XR layout at
`$XDG_CONFIG_HOME/dome/xr.json` is used. Session logs are in
`$XDG_STATE_HOME/dome/logs` (default `~/.local/state/dome/logs/latest.log`).
Run `dome-session --check` to verify installation dependencies.

Native logins save open applications and window placement in
`$XDG_STATE_HOME/dome/session.json` (normally `~/.local/state/dome/session.json`).
The next login reopens applications using their saved argument boundaries and
working directories, then restores workspaces, lane order/rotation, desktop
geometry and stacking, minimized/maximized state, and XR poses, scale, dome offsets and
pullout placement. XR presenters recreated later by their owning applications
retain their placement even when the headset starts after login. Transient
dialogs and popups are excluded; document recovery remains application-owned.
The file is private and written atomically, with background checkpoints and a
final save before logout, restart, shutdown or compositor termination.
`DOME_SESSION_RESTORE=0` disables both saving and restoring. Nested sessions leave
the saved login untouched unless explicitly enabled with `DOME_SESSION_RESTORE=1`.

For real KMS regression tests without using the current seat, prepare a private
VKMS card, then run the native tests. They exercise actual kernel page flips,
Wayland and EGL client pixels, seat pause/resume, held-button release, inactive
input suppression and clean exit. No physical input devices are opened.

```sh
pkexec /usr/bin/bash "$PWD/scripts/vkms-test-device.sh" prepare
uv run --no-project --with PySide6 --with pillow python scripts/test-native-drm.py
uv run --no-project --with PySide6 --with pillow python scripts/test-native-drm.py --installed
pkexec /usr/bin/bash "$PWD/scripts/vkms-test-device.sh" prepare-second-gpu
uv run --no-project --with PySide6 --with pillow python scripts/test-native-drm.py --multi-gpu
pkexec /usr/bin/bash "$PWD/scripts/vkms-test-device.sh" cleanup
```

`DOME_DRM_TEST_DEVICE` and the comma-separated `DOME_DRM_TEST_DEVICES` accept only
the kernel's `vkms` driver. Its optional
`DOME_DRM_TEST_CONTROL` socket is available only in that test mode. Production
sessions always use libseat and libinput. `DOME_DRM_CAPTURE` and `DOME_DRM_REPORT`
write diagnostic framebuffer snapshots and scanout counters when explicitly set.
`DOME_DRM_CAPTURE_ALL=1` names snapshots per output; `DOME_DRM_REPORT_ALL=1`
names reports per GPU. The native tests cover two heads, unplug/replug, negative
coordinates and a client moving between two virtual GPU devices.

## Compositor renderer

DomeD defaults to `--renderer vulkan` (`DOME_RENDERER=vulkan`). Smithay's GLES
renderer runs through [Mesa Zink](https://docs.mesa3d.org/drivers/zink.html), which
executes composition on Vulkan. This is a Vulkan-backed GLES implementation,
not a separate native Vulkan implementation of Smithay's renderer traits.
Mesa Zink and a Vulkan driver for the GPU are required. Startup logs the actual
renderer and rejects a Vulkan request if the driver is not Zink.

Native DRM composition renders to GBM scanout buffers. Nested mode uses an
independent Vulkan context for composition and copies the finished frame through
a GLES presentation context, so the host display need not provide Vulkan WSI.
The nested copy is intended for development; it is not a zero-copy path. Driver
selection is confined to compositor context creation and does not override the
renderers of launched applications. `--renderer gles` or `DOME_RENDERER=gles`
selects the previous GLES path. `DOME_RENDER_CAPTURE=/path/frame.ppm` enables
nested framebuffer readback for diagnostics.

The keyboard respects `[Keyboard] NumLock` in `kcminputrc`: `0` enables NumLock,
`1` disables it, and `2` (or an absent setting) preserves the existing kernel LED
state when available. NumLock and CapsLock survive keyboard layout changes, and native
keyboard LEDs follow NumLock/CapsLock/ScrollLock, including newly added devices.

## Clipboard, KWallet and KDE Connect

DomeD supports regular Wayland copy/paste, primary selection (middle-click paste),
and both `zwlr_data_control_manager_v1` and `ext_data_control_manager_v1` for
unfocused clipboard tools. Unicode text and arbitrary MIME bytes work. Clipboard
history/persistence requires a separate clipboard manager. KDE Connect's current
KSystemClipboard backend uses ext-data-control and is tested in both directions.

The native login session exports its ready Wayland display and Qt platform to D-Bus
before starting wallet integration and the installed KDE Polkit authentication
agent. This keeps administrative prompts available without Plasma autostart;
`DOME_START_POLKIT=0` disables agent startup. When a PAM wallet socket is supplied, DomeD
runs the installed `pam_kwallet_init` helper. Current KWallet's `ksecretd` starts
when no Secret Service provider already owns the session bus; KDE applications can
activate `kwalletd6` normally. The Dome-specific portal configuration selects
KWallet for the Secret portal and GTK for common desktop portals. Existing wallet
settings and login-manager PAM configuration are preserved. Automatic unlocking
requires PAM login credentials; passwordless login can still require a wallet
password. The tests use a separate bus, HOME and encrypted disposable wallet and
verify the real password dialog, Secret Service write/read, and KWallet API.

KDE Connect's indicator starts automatically at login when `kdeconnect-indicator`
is installed, after the session's Wayland display is ready. Set
`DOME_START_KDECONNECT=0` to disable this startup. Its daemon/CLI and graphical
interface can also run inside DomeD. DomeD does not yet run general XDG autostart
entries. Remote phone mouse/keyboard control is currently unavailable because DomeD
has no Remote Desktop portal backend. DomeD now supplies the notification server
and action buttons used by KDE Connect, including pairing requests. Device pairing,
network file transfers and phone-side operation require a real-device test. The
local audit starts KDE Connect in a private network namespace, so it never sends
pairing, discovery or payload packets to existing devices. File chooser support is
provided by the GTK portal; SSHFS and PipeWire remain external dependencies.

To repeat the service checks (after preparing local Qt development dependencies):

```sh
scripts/build-service-fixtures.sh
uv run --no-project --with dbus-next --with python-xlib --with pillow python scripts/test-session-services.py
```

## Notifications and keyboard layouts

DomeD owns `org.freedesktop.Notifications` on its session bus when no other desktop
notification server is present. Standard Notify, replacement, CloseNotification,
GetCapabilities and GetServerInformation calls are supported. Desktop toasts and
a separate headset-facing XR pane share the same dismiss and action handling.
Notifications preserve keyboard focus. Timeouts emit the standard close reasons;
critical notices and zero-timeout notices stay until acknowledged. Default and
explicit actions emit ActionInvoked; resident actions keep their toast open.
Up to three toasts are displayed, with a bounded queue of 128 active notifications.
Body text is plain text; sound, image hints, markup, inline reply, and persistent
notification history are not advertised or implemented. Another desktop's existing
notification daemon is never replaced by a nested DomeD test.

US English, **Anthy** Japanese, Czech, Korean **Hangul**, and Serbian Cyrillic
input use the installed Fcitx5 framework. **Super+Space** cycles the available
methods; **Super+Shift+Space** opens the application launcher. Clicking the
top-bar US/あ/CZ/한/СР indicator also switches input
methods. The native login session starts Fcitx5 on a private, trusted Wayland
connection. Input-method-v2 and virtual-keyboard privileges are restricted to that
connection. Text-input-v3 clients receive Japanese preedit, conversion and commit;
candidate popups use the same desktop and spatial popup pipeline as application
popups and retain the text field's keyboard focus.

Fcitx5's Dome-specific configuration lives under `~/.config/dome/fcitx5/`, with
`keyboard-us` and `anthy` seeded when no Dome profile exists. Startup adds installed
`keyboard-cz`, `hangul` and `keyboard-rs` choices to the active Dome group while
preserving existing entries, order and layout overrides. Serbian uses the default
Cyrillic XKB layout; Hangul uses Korean syllable composition. Another session's
Fcitx5 instance and configuration are preserved. GTK applications use the Fcitx
module; Qt6 uses native Wayland text input. Applications need a compatible text
input protocol or toolkit module. Fcitx5, `fcitx5-anthy` and `fcitx5-hangul` supply
the complete set; unavailable engines are omitted from the switch cycle.
Nested tests start the framework only with `DOME_START_IME=1`; a normal nested
preview leaves the host input-method service alone.

Open **Dome menu → Keyboard Layouts…** to edit up to four comma-separated XKB
layout names, positional variants, options and switching shortcut. The panel also
selects whether the shortcut switches input methods or XKB layout groups.
Click a text field, use Ctrl+A to clear it, and Enter to apply or Esc to cancel.
Invalid keymaps leave the working configuration intact. Keyboard preferences and
the last XKB group save to `$XDG_CONFIG_HOME/dome/keyboard.json` (normally
`~/.config/dome/keyboard.json`). The panel uses the fitted view pane in XR.

```sh
uv run --no-project --with PySide6 --with python-xlib --with pillow python scripts/test-input-method.py
```

## Native Rust XR frontend

Start `dome-compositor` and connect native Wayland clients first. With an active
OpenXR runtime (Monado or a connected WiVRn headset) and Vulkan available:

```sh
cargo run --locked --features xr --bin dome-xr -- --server http://127.0.0.1:49640
```

Use the compositor's printed port if it differs. `XR_RUNTIME_JSON` selects the
runtime for this process. No runtime registration or service settings are changed.
The default XR background is transparent: Dome submits window/shell/popup layers
without a desktop background or opaque full-view layer. When `XR_EXTX_overlay` is
available, it creates an overlay session above the main XR application (placement
10 by default; override with `--placement N`). It prefers alpha environment
blending, then additive blending. This preserves the running application's scene
or runtime-provided passthrough behind the desktop. See the
[OpenXR overlay extension](https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html#XR_EXTX_overlay).

Without overlay support, Dome can use a transparent primary session if the runtime
supports alpha/additive blending. Camera passthrough must be enabled through the
headset/runtime; Dome does not start camera capture. If only an opaque primary
session is possible, normal startup reports that transparency is unsupported.
Use `--standalone` explicitly for an isolated diagnostic/simulator session that
may have an opaque background. Startup logs and `--report` record the chosen
overlay and environment blend settings. Client windows and shell bars keep their
own content opacity; the XR environment between them stays clear.

`--frames 240 --report /tmp/dome-xr.json --capture /tmp/dome-textures` runs a finite
session and writes diagnostics and **client texture** PNGs. Those PNGs are not eye
captures. The integration test separately captures the simulated headset output.

In **room dome** mode, attached windows use open cylindrical OpenXR layers.
Only the horizontal direction curves; vertical texture spacing stays linear,
without convergence or a top/bottom cap. The upper lane leans inward by **8°**
for readability. Adjust **Upper dome inward lean** in XR settings (0–20°), or
set `dome_upper_tilt_deg` in the geometry profile; 0° gives a straight tube.
Ray-to-UV hits use the same cylinder and lean as rendering. Dome is the only
compositor XR projection; Ghostty XR retains its independent direct-view overlay. `XR_KHR_composition_layer_cylinder` is required for
the dome; a runtime lacking it gets an explicit error instead of a flat substitute.

Grip a dome window and pull inward by 18 cm, or press **Super+P**, to move it
into a front reading panel. There are exactly **two side-by-side slots** across
the session. The panels follow a shallow cylindrical arc at a default distance
of **2 m** (`pullout_distance_m`, configurable from 0.6–3 m). Each starts at
1.3 m of arc width and preserves the window's aspect ratio. The pair anchors in
the room at the user's position/yaw on the first pull, and stays there as the
user turns or moves their head. Mouse hits and cursor placement use the same
curved surface; a stationary mouse retains its panel UV through pullout,
return, scaling and head motion.

Press **Super+P** again to return the focused panel, or push a gripped panel
outward by 18 cm. **Return to Dome Layout** also restores its original dome
placement. The shortcut is configurable under Settings → keybindings; old
profiles gain it automatically unless Super+P is already assigned, in which
case the new action starts disabled. A third pull leaves that window on the
dome. One- and two-hand pulls work, and scaling grows panels outward from the
central gap. Open the hand between pull/return transitions.

The same Qt6 menu/tray/taskbar is rendered into two XR shell layers,
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
| Grip and move | Slide along the dome |
| Pull / push a gripped dome panel | Pull into a near slot / return to dome |
| Grip the same window with both hands and change separation | Scale the panel |

White reticles show the hand targets and turn amber during a gesture. Pinches use
palm-normalized thresholds, debounce and hysteresis; a fist takes grip priority.
Both hands must reopen after tracking loss before another interaction. Drag capture
stays with its original window outside the window bounds. Tracking/focus loss,
large tracking jumps and an expired input sender release buttons and cancel grabs.
Return a window to its lane with **Shift+Super+R**.

### Spatial application popups

Wayland `xdg_popup` menus, nested menus and parented `xdg_toplevel` dialogs are
separate panels attached to their owning window. They do not take a lane slot.
Their placement follows the active projection:

* **Room dome:** a flat panel 14 cm forward and 4 cm above the parent's upper edge.
  A thin blue beam connects its lower edge to the origin on the parent surface.
  The beam is decorative and never captures mouse or hand input.
* **Desktop:** a separate panel to the right with an 8 logical pixel gap, or over
  the parent when space is tight. DomeD announces this placement in the Wayland
  popup configure event so GTK, Chromium and other clients use the same coordinates
  for submenu navigation. Placement stays within the work area between shell bars.

Popup textures are captured separately from parent textures, and pointer UVs map
to the actual popup surface. Popups follow parent movement, scale, workspaces and
visibility. Protocol popup grabs support nested menus, Escape and outside-click
dismissal. Losing the owning window's focus or changing the input realm dismisses
grabbed menus. Dialogs retain their normal client keyboard behavior. Content drawn
inside a client's existing surface cannot be split into a spatial popup.

**Right-click context menus** use a shared placement and scrolling policy instead
of the general popup rules. Their bottom centre sits 8 logical pixels above the
RMB point. Direct-view menus layer 6 cm forward; dome menus layer 14 cm forward
and connect back to the exact click with a beam. Desktop placement clamps to the
work area, and XR placement fits between the view-relative shell bars. Context
menus receive keyboard focus as soon as they map, including clients that do not
request an explicit popup grab. Other application popups keep the rules above.

Menus taller than the work area have fixed top/bottom scroll controls with a
bounded content viewport. Click the arrows or use the mouse wheel to reach hidden
content. Scroll-control clicks stay inside DomeD; clicks in the menu body map back
to the correct client content coordinates after scrolling. The same viewport
logic handles DomeD task context menus, where Up/Down, Home/End and Page Up/Down
navigate or scroll, Enter activates the selected item, and Escape dismisses the
menu. Keyboard focus stays with the menu while it is open. Native application
menus continue to use their application's keyboard handling.

```sh
./scripts/build-local.sh
python3 scripts/test-popups.py
```

The isolated test requires PySide6, python-xlib, Pillow and Xvfb. It checks real Qt
Wayland menus/dialogs, nesting, side-panel and front-fallback desktop pixels,
separate XR textures, XR pointer coordinates, dismissal and cleanup. Captures and
protocol logs are saved in `artifacts/popups/`. Rust geometry tests cover both XR
projections, beam endpoints, matching ray hits, nested placement, parent scale and
visibility, and hand input. Headset appearance still needs visual verification.
The test also checks RMB anchors in desktop and XR seat input, immediate context
focus, top/bottom scrolling, scrolled input coordinates and a task menu with fifty
desktop actions. Context screenshots are saved alongside the popup captures.

The simulator has no tracked hands: joint/gesture logic is covered by Rust tests
and injected seat events by real Wayland-client tests. Physical hand ergonomics
still need headset testing. Controller bindings, GPU buffer sharing and persistent marker localization remain follow-up work.
The optional Godot viewer remains a separate design prototype.

## XR routing, cursor ownership and desktop session

XR surfaces contain client content without compositor titlebars or borders; the
focused window expands **4 logical pixels on each edge**, filling four pixels
of the adjacent gap. A 1 mm depth bias keeps raised edges in front. Global menus
own window commands. Applications that draw controls into their own client buffers
retain those controls. Menu-exporting applications provide their actual command
trees in the global bar; applications without an exported menu keep their controls
in their own windows.

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
The nested backend and the initial native DRM backend expose one primary output.

A physical mouse motion, button or wheel event takes exclusive XR cursor ownership,
releases hand buttons and cancels gestures. Movement starts at the last laser target.
Buttons retain mouse ownership while held; after release and **800 ms** without
mouse input, hand control resumes with neutral gesture rearming. The delay is
configurable. Wheel events target the hovered surface. The mouse uses one continuous
angular coordinate across bars, lanes and empty space, with a cursor placed on the hit surface, following its curvature and lean. A held button keeps its original target for
continuous drags. Head and carousel movement preserve the cursor’s surface UV without extending the
mouse ownership timeout. Outside XR, mouse control uses the normal desktop stacking path.

In dome mode the upper menu/tray bar borders the top lane and the taskbar borders
the bottom lane, at the dome radius. Both orbit the dome anchor with horizontal
headset gaze; pitching the headset lets you look up/down to reach the bars without
moving them over the room-anchored lanes. Expanded menus keep the bar edge fixed and extend toward the lanes.
For a dome taller than the headset's field of view, look up/down to see its bars.
Dome bars use the configured dome radius and span. Adjust **Dome height / vertical
span** and **Dome height / anchor Y** in settings to change the dome’s size and elevation.
Older profiles with `in_view` automatically load as `room_dome`.

The system menu includes **Start XR**, **Stop XR**, **Appearance**, **Lock Screen**,
**Switch User**, **Sleep**, **Log Out of Dome**, **Restart** and **Shut Down**. Build
`dome-xr` alongside the desktop binary for Start XR. Lock delegates to the session's
screen locker; switching delegates to the display manager; sleep/power commands
use logind and its authorization policy. Unsupported commands are disabled. Lock,
switching and confirmed sleep/power requests also stop XR presentation. Logout
closes Dome. In nested mode it preserves the host login; in the installed login
session it releases DRM/input ownership and returns to SDDM. Dome has its own
DRM backend; logind/libseat supply session ownership. No secure screen locker is
implemented yet, so Lock is offered only when an external locker is available.

Desktop titlebars use Workbench-style gadgets: close on the left, zoom and depth
on the right, with square glyphs and raised/pressed bevels. Zoom toggles between
two size/position presets (initially the monitor work area and original geometry),
remembering changes made in either state. Depth sends a front window behind the
others, or brings a rear window forward; Shift-depth always sends it behind.
Depth ordering remains independent of keyboard focus and survives client repaints.
Gadgets act on release; dragging off cancels the click. Minimize remains available
through the Window menu, taskbar and Super+M. Border resizing and Shift-drag tiling
continue to work. These gadgets apply to desktop frames; XR client panels retain
their existing presentation.

Appearance offers a light/dark colour scheme, accent, desktop background, chrome opacity, global bitmap font and native pixel size, and
mouse-to-hand idle delay, persisted in `$XDG_CONFIG_HOME/dome/appearance.json`.
Window titlebars and borders follow the selected theme, accent and chrome opacity;
existing windows repaint immediately, including inactive decorations. The global
menu bar and taskbar use the same colours and opacity, in desktop and
XR. Global menu and status dropdowns share that palette.

The global font applies to all Dome-owned menus, panels, dialogs, notifications
and window captions in desktop and XR. The picker offers only fixed-strike fonts
with a Unicode character map and Latin text coverage, with each font's available
native pixel sizes from 8–24 px. Outline and symbol-only fonts are excluded.
GNU Unifont's 16 px Unicode bitmap face is embedded as the default, including
Japanese glyphs. DenkiChip Hangul at 12 px and Galmuri 7/9/11/14 bitmap-only faces are
also embedded as Japanese/Korean options; their native strikes are offered
without substituting their outline versions. Cozette 1.30.0 is also embedded
as a native 13 px bitmap option. Installed PCF, BDF and OTB fonts are discovered at compositor
startup, including user fonts in the usual data directories. No system Fontconfig
policy is changed. Settings save automatically; existing chrome repaints when
the font changes. Qt and GTK application controls inherit the session font, size,
light/dark mode and accent. Existing toolkit windows update when these settings
change; application-specific editor and document preferences still apply.

The login session publishes a desktop Settings portal and generates private GTK
themes and a Fontconfig configuration under its runtime directory. A GTK bridge
also updates portal dialogs. Other desktop sessions' settings are preserved.
Qt applications need their GTK platform-theme plugin (`qt6-gtk-platformtheme` on
Ubuntu) and the GTK3 runtime to receive the shared palette and font.

`scripts/test-toolkit-appearance.py` checks real Qt, GTK3 and GTK4 windows launched
through Dome, live appearance changes, portal-dialog settings, and subsequent
launches. Build `scripts/toolkit-client.cpp` with Qt6Widgets into
`artifacts/toolkit-appearance/qt-client` first; the test needs Python Xlib, Pillow,
GTK introspection and Xvfb. Its report and screenshot go to that same directory.

**Default Applications…** is available from the system menu and the Appearance
pane. It covers folders, text/Markdown/JSON, PDF and Office documents, common
image/audio/video formats, archives, and web/email links. Select a file type,
choose a registered application, and confirm **Use selected application**.
Arrow keys navigate, Enter opens/applies, Escape goes back, and the mouse wheel
browses the list. Refresh reloads the available handlers. Missing handlers and
save errors are shown in the pane.

Associations are saved atomically in `$XDG_CONFIG_HOME/dome-mimeapps.list`,
the standard desktop-specific MIME preferences honored by GIO and file-opening
tools in Dome. Other desktop sessions retain their own defaults. When installed,
`fsvr` (or `fsx-vr`, then the legacy `fsv-xr` name) is preferred for folders;
an explicit Dome folder choice takes precedence. **Open Files**, Super+E and
the FileManager1 service use that choice with the requesting Wayland realm.
`DOME_FILE_MANAGER` remains an explicit launch override.

[Miro PDF](https://github.com/vincent-uden/miro) is the recommended optional PDF
viewer. When installed, Dome selects `miro-pdf` for PDF files unless a Dome PDF
choice already exists. Without Miro, choose and install a viewer yourself in
Default Applications; Dome does not select a replacement. Other desktops' PDF
preferences are preserved.

`scripts/install-miro.sh` builds and installs Miro 0.12.0 into `~/.local` from
the verified upstream revision with Dome's appearance patch. It requires Rust,
a C/C++ toolchain, Clang headers, and Fontconfig/GLib development packages.
In a Dome session, Miro follows Appearance's light/dark setting on startup and
while open, for both its interface and PDF pages. Its saved preferences remain
unchanged and continue to apply outside Dome. See
`native/toolkit-hooks/README.md` for patch provenance.

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

Window surfaces follow an open vertical cylinder, with horizontal curvature
`1 / radius` and straight vertical edges. `dome_vertical_span_deg` specifies
unwrapped cylinder height: physical height is radius × span in radians. Upper
lane lean rotates its screens about their lower centre edges and preserves
texture aspect ratio; it does not add a ceiling.
Enlarging the dome preserves reference card height while shrinking angular sizes.
Ghostty XR retains its independent flat overlay. Focus changes the same geometry used by lasers
and mouse hit testing; its visual outset is not saved into gesture transforms.

## Application menus, global bar and taskbar

The application catalog reads standard XDG data directories and recursive desktop
file IDs, with user overrides/hidden tombstones, locale-aware names, icons,
categories, keywords, visibility rules, TryExec and Desktop Actions. GIO performs
launching and field-code expansion. A launcher request also restores and focuses
a matching existing window through DomeD's window policy, including applications
whose toolkit omits an activation request. The system menu remains available at the left.
The bold application menu follows focus and provides application identity, desktop
actions, hide and quit commands. The focused window's exported menus replace
Dome's generic File/Edit/View/Window/Help menus. Windows without an exported menu
show the system and application menus rather than another application's commands.

KDE's `org_kde_kwin_appmenu_manager` Wayland protocol associates each window with
its `com.canonical.dbusmenu` endpoint. Dome announces the standard AppMenu registrar
when no host owns it, reads menu updates off the rendering thread, and supports
nested menus, shortcuts, disabled/hidden items and check/radio state. Focus changes,
dialog parent menus, exporter removal and desktop/XR routing share the same menu
selection and hit testing. Toolkits must export a native menu; Dome cannot extract
commands drawn privately inside an application's content. Qt
native menu export is discovered through the registrar; both the default Qt theme
and the KDE platform theme work without changing Dome's desktop identity. The right side hosts StatusNotifier icons and a clock;
Dome uses an existing status watcher when present and creates one only when absent.

GTK applications use the `gtk_shell1` Wayland handshake and `org.gtk.Menus` /
`org.gtk.Actions` exports. DomeD announces that the shell owns application menus
and menu bars, reads their live models, and dispatches actions on the focused
application. Login sessions enable an installed `appmenu-gtk-module` for legacy
GTK3 menu bars while preserving existing GTK modules. If that module omits its
surface address, DomeD can use an unambiguous export from the surface client's
verified process; it never guesses between different windows' exports.

DomeD configures application popup positions before mapping them, including
submenus created before their parent's first commit. Drawing, hit testing and
client submenu navigation use those configured coordinates. Ended popup grabs
are released before another input event; clicking an ungrabbed menu keeps the
application's keyboard focus. `DOME_MENU_TRACE=1` logs menu routing diagnostics.

Native FLTK 1.4 and SDL3 popups use the same desktop/XR policy. DomeD advertises
its seat before the GTK shell extension for FLTK compatibility, and fixed-size
clients' committed dimensions are reflected in scene/XR coordinates. FLTK 1.4.4
also needs a backend fix to stop reopening menus after compositor dismissal;
`scripts/prepare-native-toolkit-popups.sh --fetch --install-runtime` builds and
installs a user-local compatibility core enabled by the next DomeD login.
`DOME_FLTK_HOOKS=0` disables that runtime. See
[the hook documentation](native/toolkit-hooks/README.md) and
[toolkit coverage](docs/TOOLKIT-INTEGRATION-AUDIT.md). These popup fixes do not
invent application menu exports: FLTK/SDL applications still need an exporter
to publish their commands in the universal menu.

The taskbar follows Plasma-style behavior: group by application, activate/minimize
single tasks, choose windows through live thumbnail previews, wheel-cycle a group,
middle-click for a new instance, and right-click for desktop actions, pin/unpin and
Close All. Pins use desktop-file IDs and persist in
`$XDG_CONFIG_HOME/dome/taskbar.json`. Launcher search remains at the left and the
virtual desktop manager at the right.

The Launch menu browses the entire visible XDG application catalog through an
eight-row viewport. Hold a cursor key to keep scrolling; selection wraps at both
ends. Home/End jump to the first/last match, and typing or Backspace filters the
catalog and resets selection. Enter runs an exact executable name from PATH even
without a desktop entry. Using cursor keys to select a menu match makes Enter
launch that desktop entry instead. Failed launches leave the menu open with an
error so the query can be corrected.

`scripts/test-launcher.py` exercises real held-key navigation, filtering, scrolled
mouse selection and PATH launching. `scripts/test-session-restore.py` exercises
logout/login with actual desktop/XR clients, private saved arguments and a late
XR presenter. Both use a private Xvfb display and D-Bus session; run them with
Python Xlib/Pillow (plus PySide6 for session restore), setting `DOME_XVFB` if needed.

Task previews use separate GPU-scaled captures capped at 126×76 pixels, with a
shared refresh budget. Unchanged surfaces reuse their cached images; hovering
does not request full-resolution XR readbacks or continuously repaint the shell.
`scripts/test-task-previews.py` checks four large grouped windows, live updates,
dismissal, and keyboard responsiveness while the previews are open.

## Gaze pointer and click gestures

In **XR settings**, set **XR pointer** to **Gaze + pinch clicks** to look at
windows and shell controls instead of aiming a hand ray. **Gaze smoothing**
controls the adaptive eye filter. These preferences persist in
`~/.config/dome/xr.json` as `xr_pointer_mode` (`hand_ray` or `gaze`) and
`gaze_smoothing`. Existing profiles keep hand rays by default.

The interaction follows the local KWin VR build: tracked eye gaze points when
available through `XR_EXT_eye_gaze_interaction`; otherwise the headset's forward
direction points. Eye movement is smoothed relative to the head, so turning your
head stays immediate. Invalid eye tracking falls back immediately and requires
80 ms of stable samples before resuming. The cursor follows the same curved or
flat surface used for input.

Either hand can pinch thumb + index for left click, thumb + middle for middle
click, or thumb + ring for right click. Hold a pinch to drag; the drag remains
on its original surface until released. Both hands can hold the same button;
it releases when the last hand releases or loses tracking. Fists retain the
existing window move and two-hand resize gestures, targeting the gaze point.
Physical mouse takeover, focus loss, tracking-source changes and interaction
mode changes release held input; open your hand before clicking again.

## Audio tray controls

Click the speaker in the right-side tray to control PipeWire audio in desktop or
XR. Speaker and microphone volume each have a slider, 5% step buttons and a mute
button. Select an output or microphone from **Default devices** to change the
WirePlumber default. Changes made by other applications and device hotplug are
reflected automatically; controls reconnect when the audio service returns.

Wheel over the speaker icon to adjust output volume without opening the pane.
Within the pane, Up/Down selects a control or device, Left/Right changes the
selected volume, Enter/Space toggles mute or selects a device, and Escape closes.
The tray controls cap volume changes at 100%; they leave application function
keys available. `pw-dump` from PipeWire tools and `wpctl` from WirePlumber are
required at runtime. Device policy and persistence remain with WirePlumber.

The audio integration test uses its own PipeWire graph, virtual devices,
policy-only WirePlumber, private D-Bus and Xvfb; it never accesses host audio:

```sh
uv run --no-project --with python-xlib --with pillow python scripts/test-audio.py
```

## Notification and clipboard history

The right-side tray includes a bell for **Notification history** and a clipboard
for **Clipboard history**. Both open the same readable history pane in desktop
and XR. Arrows, Home/End and the wheel browse entries; Delete removes an entry,
Escape closes, and **Clear history** empties the list.

Notification history retains the latest 200 notices after expiry or dismissal,
with replacement notices updating their existing entry. The bell marks unread
notifications. Clipboard history keeps up to 50 items, including text, images,
file lists and other offered formats. Select an item to restore it to the
clipboard, then paste normally. Transfers run off the compositor thread and
preserve MIME types and bytes. Capture is bounded to 8 MiB per item and 32 MiB
total; both histories last for the current session.

Tray pixmaps retain their colours, transparency and aspect ratio. Application
icon paths, themed icons and attention-icon fallback are supported.

Test the histories on an isolated display and private bus:

```sh
uv run --no-project --with dbus-next --with python-xlib --with pillow python scripts/test-shell-history.py
```

## Keyboard shortcuts

Super is the Windows/Meta key. Application Ctrl shortcuts continue to work.
The taskbar’s rightmost **Desktop / XR** button switches physical input between
views, starting XR when needed. The launcher supports cursor keys, Home/End and
Enter for selecting and launching a match.

Open **View → Displays, XR & Keybindings** (Super+Comma), select a keybinding
row and press Enter or click its value to record a replacement chord. Escape
cancels; Backspace disables that binding. Changes persist in `dome/xr.json`.
Duplicate chords are rejected. Bare function keys pass to qimgvr, mpvr, fsv-xr
and Ghostty XR: Dome’s menu uses Super+F10. F12 and Super+F12 remain reserved for
Ghostty XR’s own controls. Alt+F4 retains the standard window-close behavior.

| Shortcut | Action |
| --- | --- |
| Super+Space | Cycle input methods: English, Japanese, Czech, Korean, Serbian Cyrillic |
| Super+Shift+Space | Search and launch apps |
| Super+T | Open Ghostty XR terminal in the active desktop/XR realm |
| Alt+Tab / Shift+Alt+Tab | XR: next / previous window in the same lane, recentered; desktop: next / previous window |
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
| Super+F10, arrows, Enter, Escape | Navigate classic global menus |

XR visibility controls are also in **View**. Bar slides and desktop fades take
250 ms and reverse continuously if toggled mid-animation. The upper bar slides
past the top view edge; the taskbar slides past the bottom. Hiding the XR desktop
fades all window and shell layers, preserving each bar's visibility preference.
These controls remain available from
launchers and shell settings. Hidden/transitioning surfaces reject XR pointer
input. Once every XR panel and tray is hidden, physical mouse and keyboard input
return to the desktop compositor, including clicking, scrolling and launching apps.
Showing any panel or either tray returns physical input to XR. Each view restores
its own previous keyboard focus; held buttons/grabs are released during handoff,
and stale XR packets cannot recapture the desktop seat. Open XR shell popups and
modal input capture are dismissed when handing control to desktop. During fade or
slide-out, XR retains ownership until its last visible surface disappears.
Desktop window placement and monitor bars are unaffected. This is a presentation
toggle: the XR session stays alive and does not trigger contingency recovery.

The global bar uses the focused application's exported menu tree in both desktop
and XR, alongside DomeD's system and application controls.

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
cargo build --locked --features standalone,xr
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

## Optional qimgvr stereo image viewer

qimgvr 1.2.0 with Arc SBS 0.4.0 can serve as Dome's default image viewer. Run
`python3 /path/to/qimgvr/scripts/install-domed.py` as the desktop user to enable;
`--disable` restores tracked previous associations. Other desktop defaults are
preserved. A manual launch is `qimgvr --domed image.jpg` inside Dome.

With an active XR renderer, each image opens in a separate XR-only window while
the normal viewer stays on the desktop. Existing SBS images bypass inference;
ordinary images convert asynchronously on the A770. Only the explicit
`org.qimgvr.Sbs` app ID opts into stereo. Quad and cylinder composition submit
nonoverlapping image halves to OpenXR LEFT/RIGHT eye visibility, counting two
layers against the runtime's layer budget. Ordinary apps remain mono.

The private per-socket session record advertises `control_port` and `xr_socket`.
GET `/api/integrations/qimgvr` returns version 1, compositor PID, XR socket,
`xr_active` and the stereo app ID. Existing loopback Host/Origin checks apply.
The viewer validates Unix peer credentials and the owned session record before
using this endpoint. It does not create a headset session.

Validation: `scripts/build-local.sh --release` builds all features and runs Rust
tests. Run the isolated compositor/default-opener test with the installed Arc
Python: `~/.local/opt/arc-sbs/current/venv/bin/python scripts/test-qimgvr.py`.
It requires Xvfb, qimgvr, cv2/numpy and the local depth model. Results are written
under ignored `artifacts/qimgvr/`. It tests real Wayland windows and pixel content
using a simulated renderer heartbeat; it does not verify headset perception.

## mpvr SBS video integration

The mpv fork `mpvr` uses the same XR-only stereo composition path with
explicit app ID `org.mpvr.Sbs`. GET `/api/integrations/mpvr` exposes the same
versioned session status as the qimgvr endpoint, with mpvr's app ID. Normal mpv
windows remain mono. The compositor advertises Smithay's `wp_viewporter` protocol,
including viewport validation and surface scaling through the existing renderer,
because current mpv requires it to initialize its Wayland GPU output.

Run `mpvr movie_SBS.mkv` for automatic routing, `mpvr --domed movie.mkv` to require
DomeD, or `mpvr --xr-output=off movie.mkv` for normal desktop playback. The mpvr
repository's `xr/install-domed.py` optionally sets Dome-only video defaults and
can restore prior associations with `--disable`. Full/half SBS, eye order,
subtitle duplication and GPU decoding are handled by mpvr. Its
`tests/xr/test_domed.py` verifies actual Wayland video pixels, the stereo app ID,
XR-only placement and cleanup in an isolated compositor with a simulated XR
heartbeat; it does not claim through-headset verification.

qimgvr 1.2.1 / Arc SBS 0.4.1 retain still-image transparency. The qimgvr integration
test also checks alpha-zero margins and exact premultiplied RGBA for existing SBS
and ordinary 2D conversion. Qt supplies premultiplied Wayland pixels, and the
existing OpenXR layer flags blend them directly; no additional compositor alpha
conversion is applied. The intermediate PNG uses straight alpha.

## Command manuals and related projects

Basic manuals: [dome-compositor.1](docs/man/dome-compositor.1), [dome-xr.1](docs/man/dome-xr.1), [dome-preview.1](docs/man/dome-preview.1). After installation, use `man dome-compositor`.

Part of the [de-xr desktop](https://github.com/topics/de-xr):
[DomeD](https://github.com/jebot-git/DomeD) provides the desktop/session,
[Ghostty XR](https://github.com/jebot-git/ghostty-xr) the terminal,
[mpvr](https://github.com/jebot-git/mpvr) video playback,
[fsvr](https://github.com/jebot-git/fsv-xr) file browsing,
[qimgvr](https://github.com/jebot-git/qimgvr) image viewing,
[Arc SBS](https://github.com/jebot-git/arc-sbs) depth conversion, and
[LightXR](https://github.com/jebot-git/LightXR) the optional login manager.
Each application has its own dependencies and can be used independently where
its selected backend permits.
