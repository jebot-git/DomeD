# Rust desktop validation — 2026-10-19

The native desktop is Rust/Smithay and the native XR frontend is Rust/OpenXR.
Godot is confined to `prototypes/godot-xr/` and has no role in these desktop tests.
Qt6 supplies compositor decorations and shell painting through a small C++ FFI
adapter. The disposable **Wayland client applications** also use Qt. Window
management, input and gesture policy remain Rust.

## End-to-end desktop test

`scripts/test-desktop.py` starts a private Xvfb display and runtime directory,
launches the real `dome-compositor` binary with Mesa software rendering and connects
multiple native Wayland clients. XTEST supplies input only to that disposable X
server. The test never attaches to the user's display or clipboard. It also
advertises temporary `.desktop` entries for launcher testing, so no regular
application is launched.

The combined desktop/XR run passed **105 checks**, including assertions against actual framebuffer
pixels and events delivered to clients:

* Map two distinct surfaces, verify their colors and click to change visible stacking.
* Type into the selected client; hover-scroll another without changing keyboard focus.
* Transfer a clipboard selection between separate Wayland clients.
* Clear focus on the empty desktop and retain that state through client repaints.
* Perform client-initiated title dragging and resizing using real Wayland seat serials.
* Verify client resize acknowledgments and new buffer contents.
* Minimize, restore and maximize through the native menu and taskbar.
* Resize the compositor output and verify maximized client dimensions and pixels.
* Switch and create virtual desktops through the native taskbar manager.
* Search and launch the temporary app, open the launcher with Super+Space, dismiss
  with Escape and verify subsequent typing reaches the launched client.
* Keep launcher confirmation/dismissal key releases out of applications.
* Disconnect one client abruptly; preserve other windows and close them cleanly.
* Exercise Qt6 title dragging, left-edge resize and native minimize controls.
* Check Super+Enter, Alt+Tab, Super+M and desktop switch/move chords with real clients.
* Translate Super+C into client Ctrl+C while temporarily removing Meta.
* Capture upright live client textures for XR and inject LMB/MMB/RMB into the seat;
  verify the input lease releases all held buttons after the sender stops.
* Run the native Rust/OpenXR frontend for 540 simulated frames, present two real
  client layers and verify both colors in Monado's actual stereo framebuffer.
* Check the Rust compositor log for runtime errors.

The report and captures are in `artifacts/desktop/`. Non-fatal Xvfb diagnostics
about missing XSETTINGS and a zero-millimetre virtual monitor are expected. Qt
fixture portal-registration warnings do not come from the compositor.

## Cursor authority and Appearance

`scripts/test-cursor.py` compiles a small raw Wayland client and runs DomeD on a
private Xvfb display. The client requests a magenta cursor, a hidden cursor, an
offset hotspot and an animated cursor surface. Framebuffer assertions verify
that DomeD's arrow remains visible through those requests, button delivery,
pointer leave/reentry and client disconnection. XFixes verifies that the nested
host cursor is transparent. The real Appearance controls switch dark → light →
dark while the test checks the cursor fill, outline and XR control transport.

```sh
DOME_COMPOSITOR="$PWD/target/release/dome-compositor" \
  uv run --no-project --with python-xlib --with pillow python scripts/test-cursor.py
DOME_RENDERER=vulkan DOME_COMPOSITOR="$PWD/target/release/dome-compositor" \
  uv run --no-project --with python-xlib --with pillow python scripts/test-cursor.py
```

Requires `wayland-scanner`, `wayland-protocols`, `wayland-client` development
files, a C compiler, Xvfb and Mesa. Reports and screenshots are saved under
`artifacts/cursor-gles/` and `artifacts/cursor-vulkan/`. The Rust transport test
also checks compatibility with an older compositor's control schema and verifies
that changing reticle colours preserves its shape and opaque outline.
Both renderers passed all 35 checks on 2026-10-22; 24 XR-related Rust tests passed.

## Bugs found and fixed

1. **Stacking reverted on repaint.** Scene synchronization called Smithay's
   `map_element` on every window, which raises it even if it is already mapped.
   Registry order then overrode the user's click. Synchronization now preserves
   unchanged mappings and raises the focused window after geometry updates.
2. **Empty desktop did not retain defocus.** Input cleared the seat's keyboard
   focus but left the shared model focused. The next client commit restored the
   previous client. Empty-space clicks now update both model and seat.
3. **Launcher leaked releases into clients.** Return and Escape presses were
   consumed, but releases were forwarded after the menu closed. The shell now
   retains ownership through release, while forwarding releases of client keys
   held before the launcher opened.

## Scope

This validates a nested single-output compositor under software GLES. The simpler
`scripts/test-native.py` additionally exercises a real EGL client under isolated
headless Weston. `cargo test --locked` covers the shared scene model; the service
smoke test covers the loopback control API and XR mesh export.

This does not establish standalone DRM/libinput support, Xwayland, fractional
scaling, complete application popup grabs,
accessibility or physical XR interaction. Those remain explicit implementation
work rather than passing tests for features that do not exist yet.

## Native XR validation

Build both binaries with `cargo build --locked --features nested,xr` (Qt6 development
packages, OpenXR loader and Vulkan required), then run:

```sh
DOME_TEST_XR=1 uv run --no-project --with PySide6 --with python-xlib --with pillow --with dbus-next python scripts/test-desktop.py
```

Requires Xvfb, Monado and Mesa Lavapipe. `DOME_XVFB` can point to an extracted Xvfb.
The test uses a private D-Bus session (including disposable tray/menu services), private sockets, an unprivileged copy of the installed Monado service
and an explicit software ICD. It changes no capabilities, system runtime manifests
or WiVRn service configuration. The installed privileged service ignores Vulkan
driver overrides; its Intel GPU cannot present to Xvfb without DRI3.

`artifacts/desktop/01-native-xr-stereo.png` is the actual simulated headset window.
`xr-report.json` records 541 frame iterations (including exit), two window layers and two shell layers,
texture uploads and a clean OpenXR STOPPING/end/EXITING lifecycle. The current
renderer uses only mouse and keyboard input and requests no hand or eye extensions.
WiVRn streaming and physical headset appearance are not validated by this test.

Rust tests cover scene/layout, shortcuts, mouse cursor geometry, idle persistence,
renderer ownership, pullout slots, shell surfaces and retired profile migration.
The API smoke test and real EGL Wayland client test also pass.

The combined run also verifies a real StatusNotifier/DBusMenu action, XR-shell
synthetic mouse menu activation, cylindrical dome layers followed by flat pulled-out
layers, live grouped thumbnails, task wheel cycling, XDG Desktop Action execution,
persisted launcher pins, grouped close-all and launching a pinned app after closing
its windows. Additional actual headset captures are `01-native-xr-pulled.png` and
`01-native-xr-direct.png`; desktop captures include `08-grouped-task-previews.png`,
`09-xdg-application-menu.png` and `01-classic-status-menu.png`.

XTEST emits a wheel press and release on X11. The grouping assertion checks the
focus transition after the press, before injecting its matching release, so it
does not assume a particular number of backend wheel notifications per pair.

## Routing, mouse ownership, palette and session regression

The expanded suite launches a real application through the XR-specific Wayland
socket, verifies it is absent from the desktop framebuffer, and delivers keyboard
input while it is unmapped from desktop. It preempts all three hand buttons with
physical XTEST mouse input, holds the mouse past the idle delay, rejects hand
resets during mouse drags and verifies hand input resumes after release/idle.
A synthetic heartbeat loss and a real OpenXR renderer SIGSTOP/resume/SIGKILL verify
recovery of the same clients on the primary output. Resumption preserves recovered
desktop visibility. XR texture dimensions exclude Qt6 desktop decorations.

Framebuffer assertions compare the actual global menu and taskbar colours/alpha
in dark and light appearances. Appearance persistence/reset is exercised through
the native settings UI. Captures include `01-appearance-dark.png`,
`01-appearance-light.png` and confirmation dialogs in `artifacts/desktop/`.
The global menu's inactive labels use the same single translucent background layer
as the taskbar, avoiding opaque Qt menu-item tiles.

A private `scripts/session-client.py` owns disposable ScreenSaver, DisplayManager
and logind names on the test bus. The compositor's system-bus address is explicitly
redirected to that private bus. Tests check lock/switch-user requests and cancellation
plus confirmation of sleep/restart/shutdown. The fixture records calls and never
performs any actual power, locking, login or logout operation on the machine.

The native renderer reports a clean simulated OpenXR lifecycle and both curved
and flat client layers. The simulator still provides no hands; real headset hand
tracking, a standalone login session and imported application-internal global
menus remain outside this validation.

Pixel-layout regression tests cover 8 px horizontal and vertical gap limits over
0.6–8 m radii, mixed client sizes and 2–40 windows in one lane; exact four-pixel
focus outsets and matching ray hits; compact near-panel spacing during scaling;
and border clipping at 0/1/2 pixels. The real OpenXR texture capture is checked
for a one-pixel inset border with client content immediately inside it.

The explicit room-dome renderer regression verifies OpenXR spherical patch gaps
in all three lanes at 0/4/8 logical pixels, across 0.6/2.2/4.4/8 m radii and mixed
client widths. It also verifies a focused dome surface expands four gap pixels
on each horizontal edge. All 18 XR layout tests pass.

## XR visibility animations

The simulator validates separate upper/taskbar shortcuts and combined bar hiding,
whole-desktop fade-out/fade-in, preserved per-bar state and unchanged monitor bars.
Framebuffer captures `01-native-xr-bars-hidden.png`,
`01-native-xr-desktop-hidden.png` and `01-native-xr-restored.png` show actual
headset output. The renderer report records intermediate slide/fade frames and
fully hidden frames. Real seat fixtures verify hidden content rejects clicks and
typing to hidden XR clients, hidden presentation does not promote XR-only clients,
and visibility shortcuts work with the appearance modal open. Desktop handoff
dismisses that modal so desktop typing is not captured. Pure tests cover interrupted/reversed
animations, premultiplied fading, initial hidden state without a visible flash,
head-relative slide directions and neutral rearming after hidden gestures.

## Visibility-driven physical input routing

Real Wayland fixtures verify desktop keyboard focus restoration, typing, clicking,
hover scrolling and launcher routing while every XR surface is hidden; restored
XR-only keyboard focus with panels visible and trays hidden; upper-tray-only and
taskbar-only XR ownership; and desktop ownership with all panels minimized and both
trays hidden. A held desktop mouse button is released exactly once at XR handoff,
and its physical release is not sent to the XR client. Reset packets cannot change
a desktop launcher's launch realm, closing a desktop app cannot focus an invisible
XR-only client, and late XR packets cannot refocus or unminimize hidden clients.
Pure policy tests cover visible-surface combinations and per-realm focus fallback.

## Focused application menus, Vulkan and NumLock

Build with `--features standalone,xr`, then run:

```sh
uv run --no-project --with python-xlib --with pillow python scripts/test-global-menu.py
```

The test compiles a real Qt Wayland exporter, opens two menu-exporting windows and
a non-exporting window, activates commands and submenus, checks disabled items and
focus routing, and types on the numeric keypad with the saved KDE startup setting.
Composition runs through Zink on Vulkan (lavapipe by default for the disposable
Xvfb display); framebuffer readbacks and screenshots are saved under
`artifacts/global-menu`. `DOME_TEST_VULKAN_ICD` selects another Vulkan ICD.
`DOME_QT6_PREFIX` and `DOME_XVFB` support locally extracted development/test tools.
This does not establish physical DRM scanout or hardware performance.

`scripts/test-input-method.py` additionally exercises clicking the keyboard
switcher, US/Anthy switching, Japanese preedit/candidates and committed 日本語,
plus desktop/XR focus restoration. It also checks Ghostty XR’s Wayland window
for visible Japanese composition, cancellation, long preedit and Enter commits
while Anthy stays selected. `DOME_GHOSTTY` selects the Ghostty executable.
`DOME_FCITX5` and `DOME_FCITX5_REMOTE` may point
to isolated Fcitx5 binaries; its normal addon/data search environment still applies.

## Compositor placement and GTK universal menus — 2026-10-20

`scripts/test-gtk-menus.py` exercises real GTK3 applications launched through
DomeD's desktop-entry path on an isolated D-Bus/display. It covers native
GMenu/GAction exports, disabled and stateful actions, nested menus, the legacy
appmenu GTK module, shell priority over client grabs, restoring an existing
application, and desktop/XR popup placement and input. `DOME_TEST_GTK4=1` adds
GTK4 menu export and popover checks when GTK4 introspection is available. The
combined run passed 16 checks. Its menu-only GTK4 run uses the Cairo client
renderer; the compositor and Qt Vulkan path are checked separately.

`scripts/test-vivaldi-menus.py` uses a disposable browser profile and private
Wayland display. It verifies compositor-configured floating menus, pointer
traversal into a submenu and the real New Window action. The user's profile and
browser windows are not used. Vivaldi is a Chromium Wayland client, so its native
popup handling is checked independently of GTK's menu exporter.

The popup suite additionally covers nested Qt surfaces, scrollable context
menus, dialogs, texture capture, dismissal and cleanup. The Qt global-menu suite
passes 13 checks on Vulkan, including live updates and focused action routing.
Screenshots and reports are in `artifacts/gtk-menus`, `artifacts/vivaldi-menus`,
`artifacts/popups` and `artifacts/global-menu`.

## Default application handlers

`scripts/test-default-apps.py` uses private XDG directories and registered fixture
applications to check Dome-specific MIME defaults, real `gio open` dispatch,
fsvr/fsx-vr detection, explicit folder overrides, torrent-file/magnet defaults,
wrapped Dome Torrent recognition and literal URI dispatch, incompatible/removed handlers,
malformed preferences and write errors. It preserves other desktop defaults and
unrelated MIME entries. With `--compositor`, it additionally starts a private
D-Bus/Xvfb compositor, verifies chooser confirmation through real pointer input,
and checks Open Files before and after selecting another file manager.

```sh
DOME_XVFB=/path/to/Xvfb uv run --no-project --with python-xlib --with pillow python scripts/test-default-apps.py --compositor target/release/dome-compositor
```

Backend checks can run with `python3 scripts/test-default-apps.py` alone. The
fixture requires a C++ compiler, de-xr FLTK and GIO development files, and
`update-desktop-database`. Screenshots and the report are saved under
`artifacts/default-apps/`.

## Native FLTK and SDL3 popups — 2026-10-20

`scripts/test-native-toolkit-popups.py` passed 16 checks each with SDL 3.4.2 and
FLTK 1.4.4 on a private native Wayland display. Both clients run with no X11
display. The host compositor is nested under Xvfb; XR is exercised through the
XR session/input APIs rather than a physical headset.

Coverage includes committed fixed-size geometry, configured detached desktop
popups, actual pointer actions, disabled/checkable items, nested traversal,
keyboard navigation, Escape, shell takeover, menus staying dismissed, above-parent
XR popups, XR submenu actions, and XR shell dismissal. GTK3/GTK4's existing
16-check suite also passed after the compositor changes; the Rust suite passed
122 tests.

The stock FLTK runtime reopens its menus after compositor `popup_done`. Its
patched runtime ends the modal menu loop before ordinary backend cleanup. The
patch, pinned source builder, local installation and test commands are described
in `native/toolkit-hooks/README.md`. The compositor also advertises `wl_seat`
before `gtk_shell1` for FLTK's registry callback and uses committed fixed-size
window dimensions for scene/XR coordinates. SDL needed no toolkit backend patch.

Results and screenshots are in `artifacts/fltk-popups`, `artifacts/sdl-popups`
and `artifacts/gtk-menus`. `artifacts/fltk-stock-popups` retains the failing stock
runtime reproduction. These popup tests do not establish application global-menu
export support for FLTK or SDL.

## Czech, Korean and Serbian Cyrillic input — 2026-10-20

The release build passed all 36 checks in `scripts/test-input-method.py`, including
actual Czech `ěšč`, Serbian Cyrillic `асдљњ` and Korean Hangul `한글` text entry in
desktop and simulated XR input. English/Anthy switching, Japanese preedit,
candidate clicks, Ghostty composition/commit, and desktop/XR focus restoration
remain covered. The script also verifies that the new choices are saved in the
private Fcitx profile. The Rust suite passed 123 tests, including preserving custom
method order and layout overrides while omitting unavailable engines.

The tested packages are Fcitx5 5.1.19, Anthy addon 5.1.10, Hangul addon 5.1.9 and
libhangul 0.2.0. The indicator shows US / あ / CZ / 한 / СР and follows the active
method. The switcher follows the actual Fcitx group rather than a hardcoded
English/Japanese toggle. Startup migrates only the compositor-owned input-method
instance; another session's Fcitx service is preserved.

The regression run also restored detached placement of input-method candidate
surfaces. These have cursor rectangles rather than xdg-popup configure events,
so their placement is computed separately from already-configured application
popups. Anthy's candidate order is learned; tests select the required conversion
by its preedit and highlighted row instead of assuming a fixed candidate index.
Results and screenshots are in `artifacts/input-method/`.
