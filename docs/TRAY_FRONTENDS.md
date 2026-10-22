# Native tray service frontends

Build as the regular user with `scripts/build-tray.sh`. The companion uses the
shared patched FLTK 1.4.5 toolkit, OpenGL widgets, DomeD's live palette/fonts and
its universal DBusMenu application-menu export. It does not link Qt.

The default tray contains one **DomeD tray applications** entry. Its menu opens
**Tray applications…**, **KDE Connect…**, **Input methods…**, or **Wallets…** in
one shared manager window. All FLTK service frontends are pages of this manager;
their compatibility desktop launchers are hidden from the applications menu.
The compact three-applications-in-a-tray icon is maintained in
`native/icons/tray-applications.svg`. Its Inkscape PNG exports at 24 and 48 pixels
are embedded in the companion and supplied as SNI ARGB pixmaps, so the entry
does not depend on an installed icon theme.

`dome-tray` (or `--daemon`) registers this single tray entry without showing a
window. `--tray`, `--devices`, `--input`, `--wallets` open the relevant page.
Additional invocations route to the existing instance. Ctrl+R refreshes, Ctrl+W/Escape closes
the window, Ctrl+Q quits the companion, Alt+1..4 selects a page, and normal Tab /
arrow navigation uses the native widgets. Closing the window always clears the
password field and keeps the tray entry available, including when started with
a page option. Only the explicit File > Quit action terminates the companion.

* KDE Connect: discovery, pairing/accept/cancel/unpair, verification/battery state,
  ping, find-phone, file/URL/text sharing and plugin enable/disable.
* Fcitx5: active input/group selection, group creation/removal, method ordering,
  default/per-method layouts, and native editing of global/input/addon settings.
  Saving edits preserves configuration fields not changed in the form. Candidate /
  preedit panels retain the Fcitx engine's Classic UI; the IM engine has not been
  replaced with a new implementation.
* KWallet: wallet creation/deletion, opening/releasing access, folders, entry
  read/write/rename/delete, masked passwords and map/binary data. Wallet maps use
  the documented QDataStream-compatible UTF-16 format without a Qt dependency.
  Reads are explicit; passwords are not written to logs, state files or the
  clipboard. Daemon-owned unlock/password-change authorization can still show
  the backend's dialog; the credential storage backend has not been replaced.
* Other applications: exported DBusMenu rows, check states and submenus appear in
  native controls. The companion does not rewrite arbitrary third-party programs
  opened by those menu actions. Their X11 windows are desktop-only through the
  session's X bridge by default.

Session startup runs `kdeconnectd` and the companion. The explicit
`DOME_TRAY_FRONTEND=qt6` compatibility override retains the external Qt indicator;
`DOME_START_TRAY=0` disables companion autostart. Existing Polkit, Fcitx engine and
KWallet PAM/Secret Service startup are retained. No running desktop is restarted.

Tests (private bus and Xvfb; install Python extras through uv):

```sh
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next python tests/tray/test_services.py
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next --with python-xlib --with pillow python tests/tray/test_ui.py
DOME_COMPOSITOR="$PWD/target/release/dome-compositor" python3 scripts/test-xwayland.py
```

Optional `tests/tray/test_hardware.py` opens a short-lived native window on the
caller's display, with fake services on a private bus, and checks the actual GL
renderer. Xvfb/software UI tests do not establish hardware GPU execution.
