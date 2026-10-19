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

The combined desktop/XR run passed **93 checks**, including assertions against actual framebuffer
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
scaling, application global-menu import, complete application popup grabs,
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
texture uploads and a clean OpenXR STOPPING/end/EXITING lifecycle. Monado advertises
EXT_hand_tracking but the simulated device supplies no hands, so no hand trackers
are created. Physical gesture ergonomics and WiVRn streaming are not validated.

`cargo test --locked` passes 60 Rust tests, including scene/layout, gesture/shortcut, catalog/taskbar,
renderer ownership and curved-surface/pull-out/shell tests. They cover three-button mapping, grip
movement, two-hand scaling/release continuity, drag capture outside windows,
tracking/focus/timeout/target-loss cancellation, gesture debounce/hysteresis, fist
priority, ambiguous pinch suppression and neutral rearming after tracking loss.
The API smoke test and real EGL Wayland client test also pass.

The combined run also verifies a real StatusNotifier/DBusMenu action, XR-shell
pinch menu activation, spherical dome layers followed by flat pulled/direct-view
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
new typing, hidden presentation does not promote XR-only clients, and shortcuts
work with the appearance modal open. Pure tests cover interrupted/reversed
animations, premultiplied fading, initial hidden state without a visible flash,
head-relative slide directions and neutral rearming after hidden gestures.
