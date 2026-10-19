# Dome design contract

The native desktop and native XR application are written in Rust. Qt6 is accessed
through a small C++ style/paint adapter; Rust owns window and input policy. Godot
is limited to disposable experiments under `prototypes/godot-xr/`; production
compositor builds and launch paths do not depend on it.

Dome is a separate experimental compositor, not a WayVR/WiVRn mode. A Wayland
client belongs to one compositor process and has one window identity. Desktop
stacking and XR placement are independent projections of that identity. A focus,
minimize, close or workspace change applies to both; desktop overlap does not
imply overlapping XR windows. There is one keyboard focus. Hover can route a
scroll without transferring keyboard focus.

## Coordinate frames and headset geometry

Units at the configuration boundary are degrees and metres. Internal room space
uses +Y up, -Z forward, +X right, with unit XYZW quaternions for view poses.
The room centre, radius and yaw are independent of the current HMD pose.

The **upper tray** and **taskbar** each have an independent maximum angular width,
angular height and inset from their nearest vertical view edge. They are planar,
semi-opaque view-frame shell surfaces. Maximum width is a cap, not a requirement
to extend outside the visible horizontal FOV. The preview clips the actual width
to a small horizontal safety margin. Native XR must compute this using each eye's
asymmetric projection, and use the intersection of the two comfortable eye regions.
A symmetric vertical FOV is an initial profile for the layout laboratory, not a
substitute for `xrLocateViews` or a lens-distortion measurement.

Lane projection is switchable without destroying or reordering windows:

* `in_view`: lanes use the current view frame. With vertical FOV V, upper inset U,
  lower inset L, upper tray height T and taskbar height B, the usable vertical
  interval is `[-V/2 + L + B, V/2 - U - T]`. Two lane gaps are removed; the three
  lane shares divide the remainder. Insets and bars are reserved angular space.
  Curved patch borders use constant view pitch, so peripheral window edges do not
  bow upward/downward across the flat trays.
* `room_dome`: lanes use the room anchor. Their total vertical span is
  `dome_vertical_span_deg`, independently of the tray FOV, bar heights and insets.
  All three lanes form a large 360-degree spatial carousel. The trays continue to
  use the view frame and may visually pass in front of the room carousel.

The default shares are 23/54/23 percent. Resizing a shared border changes only the
adjacent pair and preserves total height. Windows in a lane are ordered around a
circle with nonoverlapping angular widths. New windows enter in the middle lane
at the lane's forward position and displace existing windows. Lane swaps exchange
contents and rotation; window swaps preserve the exact occupied slots. A minimized
window leaves the ring; the remaining lane recentres on its latest window.
Maximize spans the full lane envelope and preserves the projected aspect ratio;
restore returns the previous size. Horizontal and lane gaps are capped at eight
Wayland logical content pixels. Angular gaps use the smaller adjacent pixel pitch;
crowded-ring packing scales gaps and widths together. Pulled-out panel spacing
uses the same pixel cap. Legacy angular window spacing is read but no longer
controls the layout. Surface curvature is exactly 1/radius. Increasing room
radius preserves card height at the 2.2 m reference size (bounded by the lane
envelope), reducing angular window width and apparent bend. Lane centres compress
angularly as the radius increases to keep their physical spacing compact.
Direct-view surfaces stay planar.

## Workspaces and taskbar

Each virtual desktop owns three lanes, their ordering and height shares. Switching
makes its dome the nearest layer; other domes use larger radii and lower opacity.
The preview interpolates their radius/opacity over approximately 300 ms. Only the
active desktop participates in ray hit testing. Dragging around the dome, the
unfolded 360-degree view, and explicit window/lane swap controls expose its layout.

The taskbar's left edge opens an immediately focused, searchable launcher;
Super+Space is its desktop shortcut. The right edge opens the shared desktop
manager. The preview can create, rename, switch and remove desktops; removal moves
its windows to an existing destination. Native shell currently exposes switching
and creation; renaming/removal also work through the local manager. App groups
expand into a list of their windows on hover in the preview. Real texture previews
are a future step. In the live compositor, apps are launched through installed
`.desktop` entries using `gio launch`, never an interpolated shell command. The
initial backend is native Wayland only; Xwayland clients are not yet supported.

## Scene, protocol and rendering ownership

`src/scene.rs` owns layout policy and validates actions. Curved panel mesh generation
and room/view-space ray-to-UV intersection are implemented and tested there.
`src/bin/dome-compositor/` owns Smithay protocol state, seat focus, client lifetimes,
move/resize grabs and desktop rendering. Its render loop applies scene revisions
on the compositor thread. The loopback control service updates the model; it
never invokes Wayland protocol objects from its HTTP thread.

The native desktop and XR share Qt6 shell painting and Rust input policy. The
classic global bar puts the system/application/command menus on the left and
monochrome status extras and clock on the right. App identity and desktop actions
come from an XDG catalog, with GIO activation. A Rust D-Bus worker hosts
StatusNotifier items and renders DBusMenu actions without blocking the compositor.
The taskbar groups windows by XDG identity, with live previews, pins, wheel
switching, new-instance and close-all actions. Arbitrary application-internal
menu export is not yet imported. Do not replace a login session with this prototype.

The native Rust XR frontend exports rendered Wayland window trees and Qt shell
images through an RGBA bridge into Vulkan/OpenXR swapchains. Dome-attached panels
use spherical `KHR_composition_layer_equirect2` patches; direct-view and detached
panels use flat quads. Ray hits use exactly the same surface geometry as rendering.
Window state and input run on the compositor thread; HTTP work runs on a separate
XR worker and uploads finish before swapchain release. Window capture is capped
at about 20 Hz. Application popup capture, DMA-BUF import and explicit GPU
synchronization remain future work.

Two near slots are available globally. The first pull stores a world-space,
yaw-only head frame; near panels anchor at configurable depth (default 1 m),
centred either side of a maximum eight-logical-pixel gap. Slot depth stays fixed until returned.
An 18 cm inward grip displacement detaches, and an 18 cm outward displacement on
a subsequent grip returns the panel to its dome offset and original scale. The
third pull cannot evict an occupied slot. Lateral grip displacement adjusts dome
angles. Scaling near panels grows away from the central gap; both remain flat.
Slots are released on return/close; they are session state, not persistent anchors.

The menu/tray and taskbar are independent planar shell layers in the current head
frame. Their source image crops grow for menus/previews while their outer edges
stay fixed. Their hit UVs map back to the desktop shell coordinates. Runtime head
validity gates interaction; the configured lens insets and bar heights determine
vertical placement. The horizontal runtime FOV limits configured maximum widths.

Native `EXT_hand_tracking` joints feed a shared Rust gesture policy. Index, middle
and ring pinch send LMB/MMB/RMB; grip moves a window and a second hand gripping the
same window scales it. One- and two-hand pull/push gestures share the same slot policy. Hand and window ownership persist
during a pinch drag. Neutral arming, debounce, hysteresis, target disappearance,
tracking/focus loss, position discontinuities and an input lease prevent stuck
buttons and accidental reacquisition. The simulator validates textures/lifecycle;
Rust and real Wayland client tests validate gesture policy and seat delivery.

A WiVRn marker adapter will resolve a persistent marker ID to a room-from-marker
pose, combine the stored marker-from-dome offset, and update room geometry without
using HMD-relative recentering. Anchor loss must preserve the last valid pose and
report an unlocalized state; it must not silently switch projection modes. Manual
room centre/yaw and simulated head poses are available now. Marker lookup and
persistent anchor localization are not implemented in this milestone.

## Routing, recovery and input ownership

The desktop listener marks clients `desktop`; a second listener marks clients
`xr`. Each toplevel stores its origin and independent `desktop_visible` state.
Both share one Wayland server, seat, scene and window IDs. Hidden XR clients are
unmapped from the desktop Space but still configured, activated, committed,
frame-scheduled and captured for XR. Their desktop shell view is filtered. Launch
context selects the listener without changing global environment variables.
D-Bus activation that reuses an existing connection keeps its existing origin.

A dedicated renderer status lease records its generation, visibility, focus,
head pose and FOV. A two-second expiry or loss of visible/tracked presentation
promotes hidden clients to the primary desktop, unminimizes them and cascades them
inside the work area. Focus loss alone does not promote visible content. The
separate 250 ms hand-input lease releases stuck hand buttons. Recovery is one-way
until a later explicit launch: resuming XR does not hide recovered desktop clients.
Renderer-generation changes clear stale stop requests.

Physical mouse input preempts hand packets immediately. A held mouse button keeps
ownership beyond the idle delay; after release the default 800 ms delay restores
hand pointing. The XR frontend resets and requires neutral hand poses after handoff.
The compositor maps mouse movement through the current head pose and the same
curved/flat hit geometry as hand lasers, with captured UVs for drags. Wheel input
uses the same hovered surface. Control status carries the mouse cursor for native
XR reticle rendering. Focus adds a four-logical-pixel outset on each edge and a 1 mm depth bias.
The same raised bounds drive rendering and hit testing; gesture transforms remove
the depth bias before saving poses. A configurable 0–2 pixel border is drawn inside
the texture extent, defaulting to one pixel. It adds no layout or input extent.

XR-only presentation flags independently target the top bar, taskbar and whole
desktop. A shared Rust animator interpolates reversible 250 ms smoothstep
transitions. Bars translate along head-local Y beyond the configured top/bottom
view edges, including their expanded menus. Whole-desktop fading attenuates
premultiplied RGB and alpha together; fully hidden layers are not submitted.
Visibility shortcuts stay reserved while launchers/modal shell panels are open.
Render target flags gate gestures immediately; restored pointer targeting waits
until the relevant transition completes. Existing input is released on visibility
changes, new typing is blocked while hidden, and pre-existing key releases remain
balanced. Hiding presentation never changes window origin, desktop visibility,
workspace, focus or the renderer health lease. Bar flags survive desktop fades.

Shell appearance lives in a separate validated, atomically replaced configuration.
Both bars share their background, alpha, foreground, muted, selection and accent
colours. Qt remains a painting adapter. Session capabilities and actions run on a
D-Bus worker: ScreenSaver lock, DisplayManager seat switching and logind power
commands, with disabled unavailable actions and confirmation for sleep/logout/power.
Logout affects the nested compositor. Start XR launches the sibling Rust binary;
Stop XR requests OpenXR exit. Host-session authentication and standalone locking
are outside this nested backend.

## Current milestone boundary

Implemented: real nested Wayland client rendering and input, desktop stacking,
minimize/maximize/restore, shared workspace/window actions, full layout policy,
configurable geometry, two reference frames, curved mesh/ray math and an interactive
local geometry preview, Qt6 decorations/classic menus, keyboard shortcuts and
native Rust OpenXR window layers with hand input. The preview uses synthetic content even when attached to
the live compositor; it is not a screen capture or an XR renderer.

The optional `prototypes/godot-xr/` now validates the shared curved meshes through native
OpenXR using Godot and an isolated Monado simulated HMD. It uses runtime eye/head
poses, draws view-relative trays separately from room meshes and captures both
real stereo color layers. The current window surfaces are samples. This viewer
is independent of the native Rust texture and input backend. See README
for the simulated run, WiVRn preflight and retained engine teardown diagnostics.

Next: zero-copy client texture retention, controller bindings, WiVRn marker
resolution, imported application-internal global menus and native workspace-editor polish. Later: standalone DRM/libinput, Xwayland, fractional
scaling, explicit synchronization, accessibility, popup-grab completion, session
lock, output hotplug and persistent session recovery.

## References

* Smithay and the Smallvil example: https://github.com/Smithay/smithay/tree/v0.7.0/smallvil
* Wayland/xdg-shell protocol: https://wayland.app/protocols/xdg-shell
* KDE global app menu association: https://wayland.app/protocols/appmenu
* StatusNotifierItem specification: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/
* OpenXR specification: https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html

Design references: [XDG Desktop Entry Specification](https://specifications.freedesktop.org/desktop-entry/latest-single/),
[Apple menu bar guidance](https://developer.apple.com/design/human-interface-guidelines/the-menu-bar),
and [Plasma Task Manager behavior](https://userbase.kde.org/Plasma/Tasks).
