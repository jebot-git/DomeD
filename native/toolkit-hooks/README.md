# FLTK Wayland popup dismissal hook

FLTK 1.4.4's Wayland `popup_done` callback hides its menu windows but leaves the
modal menu loop active. The loop shows every hidden menu again on its next
iteration. This reproduces when DomeD takes control through its shell menu.

`fltk-1.4.4-popup-done.patch` ends that loop with no selected action before the
ordinary backend hide/cleanup. It affects compositor dismissal of native menu
windows, including nested menus, without synthesizing application keystrokes or
changing tooltips. The patch applies to the pinned upstream release, without
public ABI changes. It does not export an FLTK application menu globally.

Build and install a user-local native Wayland runtime with:

```sh
scripts/prepare-native-toolkit-popups.sh --fetch --install-runtime
```

The script downloads Ubuntu development packages into `.local-deps`, verifies
the upstream source archive, builds the modified shared core, and installs only
`~/.local/lib/domed/fltk/libfltk.so.1.4`. It does not replace distribution files.
DomeD's login script enables this directory for session applications when present.
`DOME_FLTK_HOOKS=0` disables it. Statically linked/bundled libraries that ignore
the loader path require the source patch in their own build. This runtime is
Wayland-only; its scope is native Wayland applications in the DomeD session.

The source archive, patch and upstream COPYING are also installed under
`~/.local/share/domed/toolkit-hooks/fltk`. FLTK is LGPL-2.0-or-later with the
upstream FLTK linking exceptions; consult COPYING in that source distribution.
Upstream: https://github.com/fltk/fltk/tree/release-1.4.4

Run fixtures with the patched runtime:

```sh
DOME_TEST_TOOLKIT=fltk DOME_FLTK_LIBRARY_DIR="$HOME/.local/lib/domed/fltk" \
  uv run --no-project --with python-xlib --with pillow python scripts/test-native-toolkit-popups.py
DOME_TEST_TOOLKIT=sdl \
  uv run --no-project --with python-xlib --with pillow python scripts/test-native-toolkit-popups.py
```

`DOME_COMPOSITOR` and `DOME_XVFB` can select the compositor and Xvfb executables.
Results, Wayland traces, screenshots and scene snapshots are saved under
`artifacts/{fltk,sdl}-popups`. Tests create private displays and D-Bus sessions;
XR interaction is driven through DomeD's XR input API without a physical headset.

## Miro appearance and universal menus

`miro-0.12.0-dome-theme.patch` followed by `miro-0.12.0-global-menu.patch`
applies to Miro upstream commit
`d35322c47d53903638558162a5e3153f32dfdc37` (version 0.12.0).
Run `scripts/install-miro.sh` to build, test and install it into `~/.local`.
The script retains the clean upstream checkout, patched source worktree,
license, both patches and revision. It pins this verified revision; review and
rebase both patches before changing that pin to a later upstream revision.

In `XDG_CURRENT_DESKTOP=Dome` sessions, Iced's standard theme query and change
subscription read the desktop Settings portal, which Dome publishes from its
Appearance settings. Both interface and PDF dark mode follow it, including
documents opened later. No Miro preferences are written. Outside Dome, Miro's
own configuration applies. The Wayland application ID is `miro-pdf`, matching
the installed desktop entry. The patch also corrects the upstream interface
toggle to use its own state rather than the PDF inversion state.

Miro is AGPL-3.0; it is separately built and is not linked into DomeD.
Upstream: https://github.com/vincent-uden/miro

`scripts/test-miro-appearance.py` opens an upstream sample PDF on an isolated
Wayland display, clicks the actual Appearance pane, and checks rendered page
pixels across dark/light/dark transitions, subsequent launches, unchanged Miro
preferences, and operation outside Dome. It launches Miro through Dome's desktop
launcher and opens another PDF through GIO in XR with desktop Miro already
running, checking the Wayland origin and desktop visibility of both windows.
Run it with Python Xlib and Pillow;
`DOME_COMPOSITOR`, `DOME_XVFB` and `DOME_MIRO` select the test executables.
Artifacts are written under `artifacts/miro-appearance`.

Stock Miro 0.12.0 draws its Linux menu with iced_aw and exports no application
menu. Dome's fallback therefore cannot expose its PDF commands. The menu patch
exports the existing `common_menu` definitions through `com.canonical.dbusmenu`
and associates the export with Iced's actual `wl_surface` using
`org_kde_kwin_appmenu_manager`. It does not register X11 IDs or infer a surface
from a process name. Each Miro process owns its own D-Bus connection and menu.

File, View and Layout include configured shortcuts, nested recent documents,
disabled document actions, live toggles and layout radios. Activation uses the
same `AppMessage` actions as the local menu. Model changes emit `LayoutUpdated`;
disabled items are checked again before dispatch. Dome merges its universal Edit
and Window controls. The local bar hides only after the Wayland association is
installed; X11 and hosts without that protocol retain the local bar. This patch
supports Miro's current single-window process model. It does not add an X11
registrar exporter or relocate menus drawn privately in Iced content.

Run the real desktop/XR menu regression test with:

```sh
uv run --no-project --with python-xlib --with pillow --with dbus-next \
  python scripts/test-miro-menus.py
```

`DOME_COMPOSITOR`, `DOME_XVFB` and `DOME_MIRO` select executables. The test uses a
private bus/display and checks native pointer and keyboard activation, rendered
PDF changes, radio state, recent-file traversal, empty-document disabled items,
multiple-process focus, XR dispatch, Window presentation, exporter disconnect,
and in-window fallback. Results and screenshots are in `artifacts/miro-menus`.
