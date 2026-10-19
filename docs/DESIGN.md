# Dome design contract

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
restore returns the previous size. The room radius scales the entire physical
layout, without changing its angular proportions.

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

The native desktop has a semi-opaque shell with app identity, compositor window
commands and taskbar. It does not yet import arbitrary application menus or
StatusNotifier icons. The global app-menu protocol adapter and D-Bus StatusNotifier
host are explicit follow-up work; the shell marks the tray as pending rather than
showing fake system status. Do not replace a login session with this prototype.

For XR, client surface trees must be rendered into textures, with popups and
subsurfaces associated with their parent. A retained texture must have an explicit
release/fence lifetime; desktop and XR consumers cannot independently release a
Wayland buffer. The next renderer will consume scene meshes and import retained
client textures into OpenXR swapchains. Controller rays and tracked-hand rays use
the same scene UV hit test and seat arbitration. Focus changes require a click;
scroll routes to the hit surface without changing keyboard focus. Loss of focus
or tracking releases buttons and grabs before changing owner.

A WiVRn marker adapter will resolve a persistent marker ID to a room-from-marker
pose, combine the stored marker-from-dome offset, and update room geometry without
using HMD-relative recentering. Anchor loss must preserve the last valid pose and
report an unlocalized state; it must not silently switch projection modes. Manual
room centre/yaw and simulated head poses are available now. Marker lookup and
persistent anchor localization are not implemented in this milestone.

## Initial milestone boundary

Implemented: real nested Wayland client rendering and input, desktop stacking,
minimize/maximize/restore, shared workspace/window actions, full layout policy,
configurable geometry, two reference frames, curved mesh/ray math and an interactive
local geometry preview. The preview uses synthetic content even when attached to
the live compositor; it is not a screen capture or an XR renderer.

Next: client texture retention and XR rendering, OpenXR input/FOV/lifecycle,
WiVRn marker resolution, D-Bus app menus and tray, real hover thumbnails and native
workspace-editor polish. Later: standalone DRM/libinput, Xwayland, fractional
scaling, explicit synchronization, accessibility, popup-grab completion, session
lock, output hotplug and persistent session recovery.

## References

* Smithay and the Smallvil example: https://github.com/Smithay/smithay/tree/v0.7.0/smallvil
* Wayland/xdg-shell protocol: https://wayland.app/protocols/xdg-shell
* KDE global app menu association: https://wayland.app/protocols/appmenu
* StatusNotifierItem specification: https://www.freedesktop.org/wiki/Specifications/StatusNotifierItem/
* OpenXR specification: https://registry.khronos.org/OpenXR/specs/1.1/html/xrspec.html
