# Xwayland and legacy X11 retirement

The 2026-11-03 target installation has no Xwayland or Xvfb server and no legacy
X11 session/tool stack. Native DomeD, LightXR, Fcitx, WiVRn, WayVR and the running
VRChat session continued operating on Wayland during removal.

The source still supports explicit Xwayland compatibility through
`DOME_START_XWAYLAND=1`. That option requires installed Xwayland and satellite
packages. The retired-host package policy excludes them; setting the environment
variable alone does not restore the removed infrastructure.

## Runtime audit before removal

The old satellite bridge was idle. A live XRes client query found only the
server, satellite window manager and temporary audit connection. There were no
application clients. Only this bridge/server pair was stopped. The display
manager, desktop, VR services and VRChat were not restarted.

VRChat and its Windows Steam bootstrap loaded Wine's native Wayland driver,
had `PROTON_USE_WAYLAND=1`, and had no `DISPLAY`. WayVR had
`xwayland_by_default: false`. Loading libX11 in a process did not establish an
X11 connection or a dependency on Xwayland.

## Accessibility without X11

The stock AT-SPI package required `/usr/bin/xprop`, so removing the utility alone
would have broken package dependencies. AT-SPI 2.60.7 was rebuilt as the regular
user from the [official GNOME archive](https://download.gnome.org/sources/at-spi2-core/2.60/at-spi2-core-2.60.7.tar.xz),
with SHA-256:

```text
92893c8188355264ba639ca3d27943437b26ef069971ea9faa4b20de40595b9a
```

The hash matched GNOME's published checksum. The release component build used:

```sh
meson setup build /path/to/at-spi2-core-2.60.7 \
  --prefix=/usr --libdir=lib64 --buildtype=release \
  -Dx11=disabled -Dgtk2_atk_adaptor=false \
  -Ddocs=false -Dintrospection=enabled
meson compile -C build
meson test -C build --no-rebuild --print-errorlogs
meson install -C build --destdir /absolute/path/to/stage
```

All compilation and staging must run as the regular user. The verified RPM
payloads were installed separately with elevated privileges, after inspection
and dependency checks.

Six x86_64 runtime/development packages were rebuilt: `at-spi2-core`,
`at-spi2-core-devel`, `at-spi2-atk`, `at-spi2-atk-devel`, `atk` and `atk-devel`.
They include regenerated GIR/typelib data for the X11-disabled API. The obsolete
Xwayland session script, X11-only header and GTK2 ATK module were omitted.
Actual ELF dependencies were regenerated; the `xprop` and X11 pkg-config
requirements were removed after disabling the upstream feature. No fake
capabilities or dependency bypasses were used.

This build removes four X11-specific exports, including the DeviceX11
constructors/type. No remaining checked native accessibility consumer referenced
them. It does not promise compatibility with arbitrary X11 accessibility clients.
The unused i686 AT-SPI/ATK/bridge packages were removed; native x86_64
accessibility remains installed. Existing accessibility helpers were allowed to
keep running and load the new binaries at their next activation/login.

## TeX and package retirement

The seventh replacement was `texlive-collection-basic`. Its epoch, virtual
provides and other requirements were retained; only its `texlive-xdvi`
requirement was omitted. TeX/LaTeX engines, fonts, other collection data and a2ps
remain available without the X11 DVI viewer.

The 36 removed package records are listed in
[`retired-x11-packages.json`](../packaging/toolkit/retired-x11-packages.json).
This is the checked target inventory, not an automatic removal script for other
systems. It covers Xwayland/satellite, Xvfb, X session/authentication setup,
legacy display/input/settings tools, the DVI viewer, unused X-only support
libraries and obsolete development/font packages.

The exact combined transaction was checked through RPM and a saved DNF solver
transaction: seven upgrades and 36 removals, with automatic dependency removal
disabled. The installed satellite wrapper and its installed-file checksum entry
were removed separately. Stale bridge socket/lock files were removed only after
verifying no listener and acquiring the lock.

## Retained shared libraries and package policy

Shared X11-family libraries remain where native software still links them:

- LightXR needs libXdmcp; its native greeter needs libxklavier.
- Fcitx and FreeRDP need libxkbfile.
- FLTK, SDL, graphics/VR and Wine components retain libX11/libxcb dependencies.
- libxkbcommon and xkeyboard-config provide native Wayland keyboard handling.
- Remaining X protocol/transport headers support shared development packages.

These libraries and headers are not an X server. Removing them would require
further source ports/rebuilds; simply deleting their packages would break native
binaries even when those binaries use Wayland.

[`no-gtk-qt.dnf.conf`](../packaging/toolkit/no-gtk-qt.dnf.conf) now includes the
retired X11 package names and protection for the seven local native replacements.
The same exclusions are installed in the DNF5 drop-in and merged into DNF4's
main configuration. This prevents an ordinary upgrade from restoring the old
`xprop` dependency or X server stack. Existing GTK/Qt and uv policy remains.

The excluded names are intentional for this retired-host profile. Any deliberate
return to X11 compatibility needs a reviewed policy/package change. Xvfb-based
test fixtures also need a separate explicitly prepared environment; they are no
longer prerequisites supplied by this host.

## Validation

- All 10 upstream AT-SPI tests passed in the private release build.
- DNF dependency checks and full root RPM verification passed for the seven new
  packages and the previous 14 native toolkit replacements.
- 24,764 installed ELF paths were inspected for consumers of removed libraries.
  Required native greeter/input dependencies were retained.
- LightXR, compositor, WayVR and Fcitx linkage had no missing libraries/symbols.
  The rebuilt accessibility libraries and bus helpers have no direct
  X11/Xi/Xtst/XRes dependencies.
- Live x86_64/i686 libdecor Cairo tests, Godot, Fcitx, GStreamer video conversion,
  CairoSVG, SDL3, and TeX/LaTeX version checks passed.
- No active Xwayland/Xvfb server remained. Native display-manager, desktop and
  VR application process IDs stayed unchanged.

Private configuration backups, package payloads and machine-specific transaction
scripts remain outside the source repository. No desktop or display-manager
restart was used to validate or install these changes.
