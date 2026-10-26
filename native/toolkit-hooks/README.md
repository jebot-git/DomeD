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

## Popup lifetime regression checks

Pointer leave alone must preserve native menus, including every open submenu
and `Fl_Choice` dropdowns. Selection, an outside click, or Escape dismisses them.
The native popup test checks desktop leave and XR leave into empty space,
outside-click dismissal even without an `xdg_popup` grab, and Escape.
Set `DOME_TEST_REAL_APPS=1` with `DOME_TEST_TOOLKIT=fltk` to additionally exercise
the installed Comic Chat character dropdown and fsvr right-click menu on the
private XR compositor. This runs temporary clients without changing the active
session. The reported immediate leave dismissal did not reproduce in these
application tests; physical-device routing still needs a live reproduction.

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

The isolated FLTK 1.4.5 build also applies `first-configure`: libdecor windows
can redraw after their first valid committed configure. Transient dialogs must
not depend on a redundant second configure or later activation to finish
exposure; otherwise native file choosers freeze at their first frame.

`fltk-1.4.5-empty-gl-text.patch` prevents the GL text cache from constructing
a zero-width offscreen surface for empty reader lines or zero-advance strings.
Empty strings return before caching; remaining zero-width glyph runs receive a
minimal allocation. Penguin Mail's native reader and composer smoke checks cover
the original crash.

`fltk-1.4.5-primary-selection.patch` adds the native Wayland PRIMARY protocol to
the shared FLTK toolkit. Selection 0 is published system-wide; selection 1 stays
the explicit Copy/Paste clipboard. Receives run asynchronously with widget
lifetime tracking, and outgoing writes retain their own byte snapshot, handle
partial writes and tolerate receiver disconnects without SIGPIPE termination.
The FLTerm-XR private integration test exercises separate FLTK processes, external
ownership changes and independence from the explicit clipboard. Reopen clients
normally after installing the shared release; never restart the desktop session.


`fltk-1.4.5-file-picker.patch` improves the shared native file chooser used by
`Fl_File_Chooser` and the FLTK backend of `Fl_Native_File_Chooser`. The dialog
provides natural name, size, extension/type and modified-date ordering, ascending
or descending, and an independent folders-first toggle. Click a column heading
to select it; click again to reverse. Parent navigation stays first and missing
metadata stays last. Details show size, type and local modification time; narrow
windows hide secondary columns. The Details toggle provides a compact name list.
Refresh/F5 reloads the current directory. Existing filtering, hidden files,
favorites, previews and folder creation remain available.

`fltk-1.4.5-file-picker-places.patch` adds the optional **Places** sidebar to
that shared picker. It shows Home, existing XDG user folders, File System,
native Favorites, local FSVR/GTK bookmarks and mounted user volumes. Connected
GVfs locations appear as devices and use their existing filesystem exports;
the picker does not mount shares or request network credentials. Remote URL
bookmarks without a local path are omitted. Duplicate paths are shown once.

Click a place to navigate; use the keyboard arrows and Enter to open it.
**Add** saves the current folder through FLTK's existing Favorites mechanism;
**Manage** opens its existing reorder/remove dialog. Imported bookmarks remain
managed by their originating applications. Refresh/F5 reloads the Places list
along with the file listing. Missing bookmarks stay listed and report an error
only when opened. XDG user-directory values are parsed without executing shell
commands. Import size and row counts are bounded.

The Places toggle persists with the picker preferences. A save dialog retains
the typed basename when switching folders; an open dialog clears its previous
file selection. The sidebar remains usable when a directory listing fails,
fits beside the optional preview, and hides if the file pane becomes too narrow.
Both the generated chooser code and FLUID source retain the hooks. The patch
preserves public headers, class layouts and existing shared-library exports.

Sorting and details preferences use FLTK's existing user filechooser preferences.
Reordering preserves the selected file set and the filename field, including a
new save filename. Metadata is collected once per directory reload, not during
comparisons or drawing. Type labels describe extensions; they do not require a
MIME database or GTK/Qt dependency. Metadata collection, like the original
listing, is synchronous; slow mounts can still delay a reload.

The patch changes no public FLTK headers or class layouts. Its controls and
metadata live in an internal browser subclass, so existing dynamically linked
applications can use the new dialog without recompilation. Applications already
running retain their mapped toolkit; reopen them normally to use the release.
Do not restart the desktop or display manager when installing it.

The generated chooser source and its FLUID source both retain the integration.
Build/test against the isolated toolkit, explicitly overriding an installed FLTK
path inherited from the login session:

```sh
cmake -S tests/file-picker -B build-file-picker -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_PREFIX_PATH=/path/to/isolated/toolkit
cmake --build build-file-picker --parallel 4
export LD_LIBRARY_PATH=/path/to/isolated/toolkit/lib:/path/to/isolated/toolkit/lib64
FLTK_BACKEND=x11 xvfb-run -a build-file-picker/file-picker-test artifacts/picker-x11
python3 tests/file-picker/test-wayland.py build-file-picker/file-picker-test artifacts/picker-wayland
```

The Wayland test starts a private nested compositor/session and never restarts
the active desktop. Tests cover natural numeric ordering, size/type/date and
reverse ordering, folder/parent placement, hidden-file reloads, saved settings,
multiselection and save-name preservation, preview toggling, minimum-size layout,
and native single-file, multiple-file, save-file and directory return values.
They also cover Places sources, Unicode and escaped bookmarks, duplicate and
malformed entries, Favorites add/remove synchronization, save-name preservation
across Places, error recovery and persistent sidebar visibility.
