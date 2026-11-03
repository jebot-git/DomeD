# GTK/Qt host retirement

The 2026-11-03 installation uses FLTK for native application controls and has
no host GTK2/GTK3/GTK4 or Qt4/Qt5/Qt6 runtime packages. Private Steam compatibility
runtimes remain intact for Steam/VR applications. GLib/GObject, GdkPixbuf, Cairo,
Pango and JavaScriptCore are separate libraries; removing GTK does not require
removing them.

This records the verified target installation and reusable package policies.
It is not an automatic removal script: local package inventories and private
application environments must be inspected before changing another system.

## Native runtime replacements

All native components were built as the regular user in optimized release mode,
staged and inspected before installing system files. No desktop or display
manager was restarted.

| Component | Result |
| --- | --- |
| libdecor 0.2.5, x86_64/i686 | Meson `--buildtype=release -Dgtk=disabled -Ddemo=false`; retain core library and Cairo plugin, omit GTK plugin. |
| System FLTK 1.4.5 | DomeD bitmap/Wayland fork; shared release libraries, bundled libdecor, `FLTK_USE_LIBDECOR_GTK=OFF`, `FLTK_USE_PANGO=OFF`; retain Forms, GL and Cairo libraries. |
| GStreamer bad-free 1.28.7, x86_64/i686 | Rebuild native Wayland components with `-Dauto_features=disabled -Dwayland=enabled -Dgtk3=disabled`; preserve other installed codecs and omit only `libgstgtkwayland.so`. |
| python3-cairocffi 1.7.1 | Disable optional GDK3 loading and use its existing GTK-free RGB/PNG conversion paths. Regenerate bytecode. |
| plymouth-devel | Retain headers and development libraries; omit the GTK-based X11 preview renderer. Boot rendering is unchanged. |
| redhat-rpm-config | Retain RPM configuration and remove requirements on `qt5-srpm-macros` and `qt6-srpm-macros`. |

libdecor remains necessary: Xwayland, Godot and SDL3 require its ABI. The Cairo
plugin works without GTK. Both installed architectures passed a live Wayland
context test with Cairo loaded, no GTK mappings and no errors.

Pinned upstream source archives:

- [libdecor 0.2.5](https://gitlab.freedesktop.org/libdecor/libdecor/-/archive/0.2.5/libdecor-0.2.5.tar.gz):
  SHA-256 `39c109a9a7eae943ba34d18a282c447d5729f9c486c8bc05ea305e4acd341522`.
- [GStreamer bad-free 1.28.7](https://gstreamer.freedesktop.org/src/gst-plugins-bad/gst-plugins-bad-1.28.7.tar.xz):
  SHA-256 `dc525383c18b2c265bbe6a43d498656cd918aaa130aa4e3abeabcdaa741c3ffe`.

The GStreamer build with automatic features disabled is a component staging
build, not a complete replacement codec distribution. Do not replace an entire
installed plugin directory with its reduced output. Final RPM payloads retained
the other codecs, regenerated actual ELF requirements, and passed dependency
resolution without bypassing dependencies or adding fake GTK/Qt provides.

The bitmap FLTK build omits 14 Pango/PDF/internal/RTTI exports from the stock
library. None were referenced by the installed native consumer ELF files checked
on the target. This does not promise Pango/PDF ABI compatibility for arbitrary
third-party FLTK applications.

## Package installation policy

`packaging/toolkit/no-gtk-qt.dnf.conf` contains the installed Fedora DNF exclusions
and protection for the local native replacements. It also incorporates the later
[X11 retirement](X11_RETIREMENT.md) exclusions and X11-free accessibility packages.
Review the native package names against the target inventory before enabling it. On the verified system
it is installed at `/etc/dnf/libdnf5.conf.d/99-de-xr-preserve.conf`, with the same
`excludepkgs` value merged into `/etc/dnf/dnf.conf` for DNF4 compatibility. Merge
with existing exclusions rather than overwriting unrelated DNF configuration.

`packaging/toolkit/no-gtk-qt.uv.toml` disables both wheels and source builds for
known GTK/Qt Python bindings and the Qt-bearing OpenCV distributions. Merge its
two lists into the user's `~/.config/uv/uv.toml`, preserving other settings.
`opencv-python-headless` and `opencv-contrib-python-headless` remain available.

DNF filtering was checked against cached GTK4/Qt6 repository candidates: normal
queries hide them, while clearing `excludepkgs` exposes them. Normal uv resolution
rejects PySide6 and the split PySide6 PDF distribution. A deliberately reviewed
local native RPM transaction can clear DNF exclusions for that transaction only.

These are package-manager policies, not a system-wide ban on arbitrary files.
They do not remove existing venvs, inspect containers or stop manual payload
copies from reintroducing private runtimes. For example, an old staged `arc-sbs`
PySide installer had to be removed separately from the current FLTK application.

## Retirement and cleanup

The installed transaction removed 55 Qt/KDE runtime/support/macro packages,
followed by a checked 25-package GTK retirement set. The latter included both
GTK3 architectures and development files, IBus/Anthy GTK frontends, Kasumi,
indicators, legacy desktop menu integration, libnma, libwnck and xapps. Native
Fcitx, its Anthy engine, libcanberra notification audio, SDL, Xwayland, Godot and
DomeD services were preserved. Xwayland was subsequently removed in the separate
[X11 retirement](X11_RETIREMENT.md); that later transaction retained the native
Wayland desktop and VR sessions.

A subsequent unused-runtime cleanup removed:

- OrcaSlicer's AppImage, desktop integration and caches; printer settings were
  backed up and print/model projects preserved. The unrelated Orca screen reader
  was retained.
- Seven unused packages: `anthy-unicode`, `ibus-devel`, `ibus-libs`,
  `javascriptcoregtk4.1`, `javascriptcoregtk4.1-devel`, `libnma-common` and
  `unity-gtk-module-common`. The separate normal Anthy package remains for Fcitx.
- 58 unowned GTK/Qt-linked system executable/plugin paths, all missing their
  toolkit dependencies: old KWin helpers/plugins, GTK/Qt utilities and the
  abandoned `resolve-real` binary.
- Seven stopped/created user toolchain containers, 40 unused images, obsolete
  GUI build/venv/sysroot/staging environments, and unused GUI package archives.
  Persistent container volumes and project sources were preserved. The separate
  root-owned container store had no containers.

Two WiVRn OpenCV environments were migrated from `opencv-python==5.0.0.93` to
`opencv-python-headless==5.0.0.93`. Stage the headless package and test it with each
environment's interpreter first. Remove the GUI distribution before installing
the headless distribution: both own `cv2` and must not coexist. Both environments
passed resize and PNG encode/decode checks, reported `GUI: NONE`, and loaded no
GTK/Qt. Models, training data and other Python dependencies were retained.

## Verification and limits

The final checks passed:

- Exact removal sets checked by RPM and the saved DNF solver transaction;
  autoremove disabled; DNF dependency check clean after removal.
- Full root RPM verification of the 14 installed native replacement packages.
- x86_64/i686 libdecor context tests, Godot version startup, native GStreamer
  Wayland inspection and a five-frame video conversion pipeline.
- Fcitx query, CairoSVG rendering, SDL3 loading and both headless OpenCV tests.
- Existing desktop and VR service processes stayed running with unchanged PIDs.

The final file scan covered `/usr`, `/opt`, the user's home, Flatpak/container
storage, system caches and `/tmp`: about 10.4 million files. The only matching
runtime library paths remaining in readable locations were in Steam compatibility
runtimes. No host toolkit libraries or unused user GTK/Qt environments remained.
Source trees, local commits and existing unrelated working changes were preserved.
Approximately 99 GiB was reclaimed.

Private recovery backups were deliberately retained outside the active runtime
scan. Some directories/process maps were unreadable without elevation; privileged
container storage was checked separately. Arbitrary embedded/static toolkit code
and every packed archive were not exhaustively unpacked. No claim is made that
all GTK/Qt bytes have been erased from the disk.
