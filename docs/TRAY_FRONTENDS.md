# Native tray service frontends

Build as the regular user with `scripts/build-tray.sh`. The companion uses the
shared patched FLTK 1.4.5 toolkit, OpenGL widgets, DomeD's live palette/fonts and
its universal DBusMenu application-menu export. It does not link Qt.

The default tray contains one **DomeD tray applications** entry. Its menu opens
**Tray applications…**, **KDE Connect…**, **Input methods…**, **Wallets…**,
**Translator…**, or **Screenshot…** in
one shared manager window. All FLTK service frontends are pages of this manager;
their compatibility desktop launchers are hidden from the applications menu.
The compact three-applications-in-a-tray icon is maintained in
`native/icons/tray-applications.svg`. Its Inkscape PNG exports at 24 and 48 pixels
are embedded in the companion and supplied as SNI ARGB pixmaps, so the entry
does not depend on an installed icon theme.

`dome-tray` (or `--daemon`) registers this single tray entry without showing a
window. `--tray`, `--devices`, `--input`, `--wallets` open the relevant page.
Additional invocations route to the existing instance. Ctrl+R refreshes, Ctrl+W/Escape closes
the window, Alt+1..4 selects a page, and normal Tab /
arrow navigation uses the native widgets. Closing the window always clears the
password field and keeps the tray entry available, including when started with
a page option. There is no Quit action: the manager is a permanent session service.
The compositor supervises its process and restarts it after a crash, with bounded
backoff for repeated startup failures. The supervisor ends with the compositor;
diagnostic sessions can still disable service startup.

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

The keyboard selector is the single language indicator. In input-method mode,
clicking its flag opens the FLTK frontend's DBusMenu: **Input methods**, **Input
method groups**, and **Configuration** submenus, plus toggle/reload actions.
Method/group radio checks follow Fcitx's actual state. The separate Fcitx
StatusNotifier item is suppressed even if it registers before the manager.
Super+Space (or the configured switch shortcut) still cycles methods; Tab,
arrows, Home/End, Enter/Space, and Escape use DomeD's shared menu navigation in
desktop and XR. Plain XKB mode retains click-to-cycle behavior.

`fcitx5-configuration-fltk` opens the input-method page in the same manager
instance. It also accepts `fcitx://config/global`,
`fcitx://config/inputmethod/ID`, and `fcitx://config/addon/ID` to open a specific
configuration. It shares the universal application menu, live DomeD palette,
bitmap fonts, Ctrl+R, Ctrl+W/Escape, Ctrl+Q, and Alt+1..4 page bindings.
Ctrl+, opens input-method configuration. Global/engine/addon settings use
Fcitx's GetConfig/SetConfig protocol and preserve fields that were not edited.
An already running Fcitx instance is observed and controlled without rewriting
its profile.

Flags use the suite's 24-unit canvas, rounded 1.6-unit outlines and a muted flat
palette. Editable Inkscape sources are in `native/icons/flags/`; run
`python3 scripts/embed-keyboard-flags.py` after editing to update the shared
shell/frontend header. Unknown engines use a neutral keyboard badge.

Session startup activates the KDE Connect backend and the FLTK companion.
Legacy `DOME_TRAY_FRONTEND` values cannot select a different UI toolkit.
`DOME_START_TRAY=0` disables companion autostart. Existing Polkit, Fcitx engine and
KWallet PAM/Secret Service startup are retained. No running desktop is restarted.

Audio, clipboard history, notification history, the clock and the keyboard
selector use the compositor's shared FLTK shell renderer. Clipboard and
notification history have visible remove, Clear history and Close controls;
keyboard navigation and deletion use the same model in desktop and XR.
DomeD has no alternate Qt renderer. Authentication and common portal dialogs
also use the installed native FLTK service frontends.

Run `python3 scripts/audit-fltk-frontends.py` after installation to check every
DomeD frontend for FLTK and reject Qt, GTK, Pango or missing shared libraries.
`--source-only` checks build and startup policy without an installation.
Third-party application windows and service engines remain separate programs.

Tests (private bus and Xvfb; install Python extras through uv):

```sh
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next python tests/tray/test_services.py
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next --with python-xlib --with pillow python tests/tray/test_ui.py
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next --with python-xlib --with pillow python tests/tray/test_keyboard_selector.py
DOME_COMPOSITOR="$PWD/target/release/dome-compositor" uv run --no-project --with dbus-next --with python-xlib --with pillow python scripts/test-shell-history.py
DOME_COMPOSITOR="$PWD/target/release/dome-compositor" python3 scripts/test-xwayland.py
```

Optional `tests/tray/test_hardware.py` opens a short-lived native window on the
caller's display, with fake services on a private bus, and checks the actual GL
renderer. Xvfb/software UI tests do not establish hardware GPU execution.

The optional [DomeD Translator](TRANSLATOR.md) is another native FLTK tray
application. The standard tray build, stage and installation scripts include it
(`DOME_BUILD_TRANSLATOR=0` opts out of building/staging it). It translates languages in the current system input group through
DeepL API Free, with independent clipboard/primary paste and local Japanese/Korean
transcription. English, Czech and Serbian do not receive transliteration.
Build it with `scripts/build-translator.sh`; it can run independently of the
service companion above.

Translator and Screenshot are permanent entries in the manager's Tray applications
list (Translator requires the installed executable or a running service). Translator
launches on demand; clipboard, primary selection, API settings and the opt-in
selection translation toggle remain available there. A running translator's live
DBusMenu is used, including its toggle state and Quit action, without a duplicate
list entry. While the manager is running, the compositor folds the translator's
panel icon into the manager; its standalone icon returns if the manager exits.

Screenshot's **Take screenshot** action uses the compositor's session-bus control
interface and the same capture path as PrintScreen, including region selection on
the desktop and stereo capture in XR. The manager hides before starting capture.
The direct PrintScreen/panel shortcuts remain available. Older running compositor
versions show a disabled capture action until the next session; installation never
restarts an active session. `dome-tray --translator` and `--screenshot` select these
entries directly, as do the corresponding manager tray-menu shortcuts.

Inside DomeD the manager exports its File/View/Input methods/Wallet commands to
the universal menu and hides the in-window menu bar, reclaiming its vertical
space. Shortcuts remain active while the local widget is hidden. Other desktop
environments retain the local menu bar as a fallback.

The manager has no applications-menu launcher. Its hidden canonical desktop
metadata (`org.domed.Tray`) supplies the correct application name and icon;
Connect/InputMethods/Wallets compatibility entries also remain hidden.

Select a tray application and use **Move to manager** or **Move to tray**. The
list marks the current placement; applications with individual icons remain in
the list so they can be moved back. Moving into the manager suppresses only the
individual icon; its live menu and process continue working. Moving out restores
the individual icon. Native service items offer **Open native controls** so
selecting them still leaves the placement buttons accessible. Placement is saved atomically in
`$XDG_CONFIG_HOME/dome/tray-applications.json` (normally `~/.config/dome`).
Translator and the optional WiVRn Pro FLTK dashboard (`wivrn.pro`) default to the
manager. WiVRn starts hidden in DomeD and exports its live settings/service menu
through StatusNotifier; selecting Show opens its single dashboard window. Both
the compositor and tray manager need rebuilding for the WiVRn default. Other applications retain their existing
individual placement until moved. Screenshot can also move between its built-in
icon and manager entry. The manager and keyboard selector have fixed placements.
If the manager is temporarily unavailable, individual application icons and the
Screenshot shortcut become visible until it recovers.

The universal menu's application header uses the resolved desktop entry's
human-readable name and icon. Exact desktop IDs take priority over window-class
aliases; unambiguous reverse-DNS client aliases can resolve a short desktop ID or
StartupWMClass (for example `com.vrcx-0.app` to `VRCX-0.desktop`). Unknown windows
use their title rather than exposing the protocol app ID as their display name.
