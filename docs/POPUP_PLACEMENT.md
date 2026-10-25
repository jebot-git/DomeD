# Popup placement

All client popup panels, nested menus and transient dialogs are bounded by the
actual inner edges of the top and bottom XR bars. They move into the available
band first and shrink uniformly when too large; rendering and input share the
same resulting geometry. Tilted upper lanes and focus outsets are included.

Dropdowns opened from the universal menu stay immediately below the originating
top bar, using the same cylindrical frame, horizontal span and texture mapping.
This preserves their relationship to their menu header. Menus opened without a
visible UI origin (for example the menu keyboard command) retain frozen pullout
placement. Application dialogs continue to use their popup placement.

Taskbar window previews and taskbar context menus extend immediately upward
from the taskbar, using its cylindrical frame, horizontal span and desktop
texture coordinates. They follow the bar as the headset turns and stop below
the upper bar. Notification placement is independent.

The `top_attached` and `bottom_attached` shell transport flags distinguish these cases. A new
compositor and XR renderer are required to activate the placement update; the
active session need not be restarted during installation.

Application rollout menus (including FLTK Choice dropdowns and cascading menus)
keep the anchor supplied by their Wayland positioner in XR. Upper controls open
downward and lower controls open upward, toward the window centre. Horizontal
placement also expands inward; nested menus preserve the client's cascade and
edge flips. These menus bypass the central pullout offset and retain a connection
to the control as the owning window moves. They do not need a decorative beam.
Dialogs and context menus without a rollout origin keep their central placement.
All popup types remain bounded between the XR bars with matching input geometry.

Rollouts are flat UI overlays, not independent spatial popup panels. They share
exactly the owning dialog's plane, orientation and pixel scale, including nested
rollouts. They do not receive extra pullout depth, density reduction or independent
bar/FOV fitting. Composition priority and workspace ordering come from the
owning UI surface, with the rollout painted after it. Coplanar pointer hits use
the same child-above-parent order. XR-only dialogs select rollout direction from
the persistent XR route, even when their asynchronous request arrives after the
input event has finished.

The `flat_rollouts` control capability keeps a newly restarted XR helper aligned
with an older running compositor until both support the flat UI geometry.
