# Native FLTK session services

These services belong to DomeD because they use its shared native GPU window,
BDF font, appearance and session runtime. Their systemd units stop with
`dome-session.target`.

- `dome-polkit-agent` presents native authentication dialogs. Polkit's own
  `PolkitAgentSession` performs PAM authentication and reports the result to the
  system authority. Password buffers are cleared, core dumps and process dumps
  are disabled, and cancellation reaches the authentication session.
- `xdg-desktop-portal-fltk` implements file selection, access consent, application
  selection, CUPS print dialogs, notification forwarding, account information,
  launcher consent, lockdown properties, and sleep/idle inhibition. The normal
  portal frontend keeps sandbox checks and document registration. Only the
  current frontend's D-Bus owner may call the backend. Closing a request or
  losing the frontend cancels pending dialogs and releases inhibitors.
- The session package keeps DomeD's existing screen-sharing and settings
  implementations and KWallet's Secret portal. It registers FLTerm as the
  system terminal and the installed FLTK Pinentry as GnuPG's default.

## Build

The build targets the installed DomeD FLTK 1.4.5 toolkit and its BDF resources.
It needs the compiler, GLib/GIO, Fontconfig, zlib, OpenGL and Polkit headers.
Polkit headers may come from installed development packages or an extracted
matching development-package sysroot; building does not require root.

```sh
python3 native/session-services/build.py \
  --build build/session-services \
  --polkit-headers /usr/include/polkit-1
```

`DOME_FLTK_PREFIX` overrides `/opt/fltk-vr/toolkit`. `--only polkit` and
`--only portal` build one service. The Polkit executable's `--preview` option
shows the dialog without registering an agent or authenticating anyone.

## Tests

Run from the DomeD repository root. These tests use a private D-Bus session and
Xvfb; they do not authenticate, send mail, contact devices or submit print jobs.

```sh
env -u WAYLAND_DISPLAY DOME_PRIVATE_SERVICE_TEST=1 \
  dbus-run-session -- xvfb-run -a -s '-screen 0 1280x900x24' \
  uv run --no-project --with dbus-next --with python-xlib --with pillow \
  python native/session-services/test_portal.py \
  build/session-services/xdg-desktop-portal-fltk build/session-services-tests

env -u WAYLAND_DISPLAY DOME_PRIVATE_SERVICE_TEST=1 \
  dbus-run-session -- xvfb-run -a -s '-screen 0 1280x900x24' \
  uv run --no-project --with dbus-next \
  python native/session-services/test_frontend.py \
  build/session-services/xdg-desktop-portal-fltk build/session-services-tests
```

The second test uses the real installed portal frontend. The current fixture
uses the caller's DomeD appearance file and installed BDF resources.

## Packaging and updates

```sh
python3 native/session-services/package.py \
  --build build/session-services --output build/session-service-packages
```

Run packaging while the distribution's `kdeconnect` package is installed. It
produces `domed-session-services` and a separate `domed-kdeconnect-backend`
package. The latter retains the distribution's daemon, CLI, transfer helpers,
protocol plugins and copyright notices, with dependencies generated from the
retained ELF files. Qt management applications and their QML dependencies are
excluded; DomeD's FLTK tray provides device management.

The backend package is maintained locally. To update it, install the newer
stock KDE Connect package, regenerate the backend package, and install the
new backend package in place of the stock package. Keep the original repository
packages available for rollback. The session package replaces Qt Pinentry, the
KDE Polkit agent and the GTK portal, so install it only with the DomeD toolkit,
font resources and existing FLTK Pinentry already available.

## Current limits

Printing supports basic CUPS jobs. Print-to-file is not implemented. Physical
printer output has not been tested. Logout and user-switch
inhibition need further compositor integration; unsupported requests return an
error. Launcher icons can be previewed as PNG or SVG, but cannot be edited in
the dialog. External parent-window attachment is not implemented.

The interface XML is pinned to xdg-desktop-portal 1.20.3; its upstream attribution
and LGPL license are preserved in `interfaces/`.
