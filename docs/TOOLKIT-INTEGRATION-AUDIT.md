# Native Wayland toolkit integration audit

Investigated 2026-10-20. The initial source audit below has a subsequent live
FLTK/SDL3 validation described at the end. This is a source audit of DomeD and upstream toolkits,
not a claim that every toolkit below has passed a live desktop/XR test. Upstream
links track development branches; packaged applications may contain older code.

DomeD owns placement, stacking, focus, and shell input priority. Prefer existing
Wayland protocols and menu exports over desktop identity spoofing or application
name exceptions. A client drawing a menu inside its main buffer must expose that
menu separately before the compositor can relocate or present it globally.

## Existing DomeD integration

- `handlers/xdg_shell.rs` configures popup placement and handles popup grabs.
  `spatial_popups.rs`, `outputs.rs`, and input routing share that placement.
- `handlers/decoration.rs` always selects server-side decorations when clients
  negotiate `xdg-decoration`.
- `appmenu.rs` accepts per-surface KDE appmenu addresses and reads DBusMenu.
  It also provides the Canonical registrar, but registrar numeric IDs alone do
  not associate a menu with a native Wayland surface. Focus selection currently
  uses KDE/GTK surface metadata and the restricted GTK discovery fallback.
- `gtk_menu.rs` supplies `gtk_shell1`, GMenu/GAction dispatch, and optional legacy
  GTK3 module activation. GTK wrappers can reuse this path where they actually
  create/export GTK menus.
- The current server has text-input-v3 and viewporter, but no
  `xdg_activation_v1`, fractional-scale-v1, relative-pointer, or pointer-constraints
  global. These are shared protocol gaps rather than per-toolkit integrations.

## Toolkit findings

| Toolkit/backend | Placement and focus | Universal menu / hook needed |
| --- | --- | --- |
| SDL3 Wayland | Native popup API uses `xdg_popup`; its configure handler applies compositor positions through `SDL_EVENT_WINDOW_MOVED`. Existing DomeD placement should apply, subject to regression testing. Test nested popups, keyboard ownership, dismissal, and libdecor offsets. | SDL supplies popup windows rather than an application menu model. Applications need a menu exporter for global menus; do not infer a menu tree from SDL windows. |
| FLTK 1.4+ Wayland | Uses `xdg_popup` and libdecor. **High-priority compatibility candidate:** its popup configure handler has special cases for tall menus and above-button placement, rather than universally updating its internal popup origin from the supplied x/y. Detached and XR positions may therefore disagree with internal menu calculations. This is a source-based risk, not a reproduced DomeD failure. | Needs a toolkit-side menu-model exporter for global menus. Test full relocation before deciding whether a backend patch is necessary. |
| EFL / Elementary Wayland | Native Wayland backend exists. Elementary widgets can draw overlays within an Evas canvas; verify which menus become separate surfaces before expecting compositor placement to affect them. | **Confirmed export gap in reviewed source:** main-menu export is guarded by `HAVE_ELEMENTARY_X`, checks `sd->x.xwin`, and registers an XID. Reuse EFL's existing DBusMenu model, adding a Wayland per-surface KDE appmenu association and enabling export on that backend. A registrar alone cannot repair the X11-only gate. |
| Chromium / Electron Ozone | Native popup surfaces can use DomeD's existing xdg handling; Vivaldi popup traversal has already been tested separately. DOM/custom window content remains application-owned pixels. | **Version/build dependent.** Current Chromium advertises global-menu support when `org_kde_kwin_appmenu_manager` exists, which DomeD already provides. Reviewed Electron code still contains an X11-named exporter/registrar path. Verify the exact runtime's export and surface association; do not blanket-disable native Wayland or assume that every Chromium-based app exports its menus. |
| wxWidgets wxGTK | wxGTK's GTK menu widgets inherit the GTK Wayland popup path. No separate DomeD placement protocol is justified yet. | GTK3 native menu bars are candidates for the existing appmenu module. Test wx callbacks, checks/radios, accelerators, and focus changes. GTK4/build differences need separate validation. Custom-drawn widgets do not automatically gain GMenu exports. |
| Eclipse SWT GTK | Reviewed SWT code uses GTK3 popup menus or GTK4 popovers. It also includes its own position-fitting logic, so test that repeated fitting does not fight DomeD's configured placement. | Reuse the GTK path where models are exported. GTK3 module coverage and GTK4 model/address publication must be checked on real SWT applications; GTK usage by itself does not establish global-menu support. |
| GLFW / winit clients | Top-level placement already belongs to the compositor. GLFW explicitly does not expose global Wayland window positioning. Winit supports startup activation tokens, exposing DomeD's missing activation protocol. | These window libraries do not establish a common application menu export. Integrate at the UI layer using them, such as a particular egui/iced application or menu library. |
| Slint winit / Flutter Linux | Slint's winit backend and Flutter's GTK host are distinct from their widget/menu implementations. A GTK host or winit window does not turn custom content into separate native popups. | Audit the selected backend/plugin. Slint has native-menu adapter machinery; Flutter uses a platform embedder. Export the actual menu model and actions, plus a surface association, where no compatible export exists. Reuse the Qt path for a Slint Qt backend when it supplies a native export. |

## Recommended order

1. Add standard **xdg-activation-v1** support and issue/propagate startup tokens
   from DomeD launches. Validate seat/serial, recency and ownership; route accepted
   activation through the same restore/workspace/focus policy as shell actions.
   The compositor decides whether to honor the request. Current launch matching
   does not replace protocol support for links, notifications or inter-app launches.
2. Add native **SDL3 and FLTK** fixtures in desktop and XR. Verify configured
   versus internal popup coordinates, nested traversal, edge placement, tall menus,
   keyboard navigation, shell priority, dismissal and reopening. FLTK is the
   first likely toolkit backend patch; SDL3 is the clearest standard-protocol
   control case. Only SDL2 development files were found locally during this audit.
3. Test **wxGTK and SWT** through the existing GTK bridge before adding adapters.
   Include a non-exporting application to catch stale menus and multiple windows
   in one process to catch incorrect action ownership.
4. Add **EFL's Wayland menu export association** on the toolkit side, reusing
   DBusMenu and `org_kde_kwin_appmenu_manager`. Keep menu routing per surface;
   do not guess using arbitrary process/window IDs.
5. Test an exact **Electron runtime** and the newer Chromium appmenu path against
   the existing KDE protocol. Patch the export/association only where absent.
6. For custom UI frameworks, provide a small optional menu adapter using existing
   exports. Separate native popup surfaces are required for compositor placement
   of menus otherwise embedded in a main window's buffer.

Fractional scaling and relative pointer/constraints should be tracked separately
for toolkit usability. Pointer lock must leave DomeD's shell escape/interaction
policy authoritative. Standard xdg-popup does not identify whether a surface is
a tooltip, menu, or combo box; any future differentiated policy needs reliable
client semantics rather than application-name heuristics.

## Primary sources

- [SDL3 popup API](https://wiki.libsdl.org/SDL3/SDL_CreatePopupWindow) and
  [Wayland configure handler](https://github.com/libsdl-org/SDL/blob/main/src/video/wayland/SDL_waylandwindow.c).
- [FLTK Wayland developer documentation](https://fltk.gitlab.io/fltk/wayland-devel.html)
  and [window driver / popup_configure](https://github.com/fltk/fltk/blob/master/src/drivers/Wayland/Fl_Wayland_Window_Driver.cxx).
- [EFL Wayland support](https://www.enlightenment.org/about-wayland),
  [Elementary main-menu export gate](https://github.com/Enlightenment/efl/blob/master/src/lib/elementary/efl_ui_win.c),
  and [DBusMenu registrar implementation](https://github.com/Enlightenment/efl/blob/master/src/lib/elementary/elm_dbus_menu.c).
- [Chromium Wayland runtime properties](https://github.com/chromium/chromium/blob/main/ui/ozone/platform/wayland/ozone_platform_wayland.cc),
  [Electron menu selection](https://github.com/electron/electron/blob/main/shell/browser/native_window_views.cc),
  and [Electron exporter](https://github.com/electron/electron/blob/main/shell/browser/ui/views/global_menu_bar_x11.cc).
- [wxGTK menus](https://github.com/wxWidgets/wxWidgets/blob/master/src/gtk/menu.cpp)
  and [SWT GTK menus](https://github.com/eclipse-platform/eclipse.platform.swt/blob/master/bundles/org.eclipse.swt/Eclipse%20SWT/gtk/org/eclipse/swt/widgets/Menu.java).
- [GLFW Wayland positioning limitations](https://www.glfw.org/docs/latest/window_guide.html#window_pos)
  and [winit startup activation](https://docs.rs/winit/latest/winit/platform/startup_notify/index.html).
- [Slint winit adapter](https://github.com/slint-ui/slint/blob/master/internal/backends/winit/winitwindowadapter.rs)
  and [Flutter platform embedders](https://docs.flutter.dev/resources/architectural-overview).

## FLTK and SDL3 implementation follow-up

Real native Wayland tests now exercise Ubuntu SDL 3.4.2 and FLTK 1.4.4, with
private Xvfb/D-Bus instances and no client X11 display. `scripts/test-native-toolkit-popups.py`
checks desktop and simulated XR popup placement, pointer and keyboard actions,
nested menus, disabled/checkable actions, shell takeover and dismissal/reopening.

Two compositor changes were necessary: advertise `wl_seat` before `gtk_shell1`
(FLTK dereferences its seat in its GTK extension registry callback), and align
fixed-size clients' scene dimensions with their committed window geometry.
Stock FLTK also reopened menus after `popup_done`: a backend hook now ends its
modal menu loop before ordinary hide/cleanup. Its source patch, pinned builder
and user-local session runtime are in `native/toolkit-hooks` and
`scripts/prepare-native-toolkit-popups.sh`. A speculative change to FLTK popup
coordinates was not needed for the cases exercised. SDL3 needed no backend patch.

These tests do not certify other toolkits, physical-headset operation, or global
menu export from stock FLTK/SDL. Neither fixture exports an application menu model.
