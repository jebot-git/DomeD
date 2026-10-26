#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="${DOME_STAGE_DIR:-$root/build/system-stage}"
release="${DOME_RELEASE_DIR:-$root/target/release}"
"$root/scripts/stage-askpass.sh" "$stage"
"$root/scripts/stage-tray.sh" "$stage"
"$release/dome-compositor" --backend drm --check-backend
for name in dome-compositor dome-xr dome-preview dome-portal-picker dome-bdf-font; do
    install -D -m 0755 "$release/$name" "$stage/usr/local/bin/$name"
done
install -D -m 0755 "$root/packaging/dome-link" "$stage/usr/local/bin/dome-link"
install -D -m 0755 "$root/packaging/dome-session" "$stage/usr/local/bin/dome-session"
install -D -m 0755 "$root/packaging/dome-session-client" "$stage/usr/local/libexec/dome-session-client"
install -D -m 0644 "$root/packaging/dome-session.target" "$stage/etc/systemd/user/dome-session.target"
install -D -m 0644 "$root/packaging/dome.desktop" "$stage/usr/share/wayland-sessions/dome.desktop"
install -D -m 0644 "$root/packaging/org.domed.Compositor.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"
install -D -m 0644 "$root/packaging/dome-portals.conf" "$stage/usr/share/xdg-desktop-portal/dome-portals.conf"
install -D -m 0644 "$root/packaging/dome-settings.portal" "$stage/usr/share/xdg-desktop-portal/portals/dome-settings.portal"
install -D -m 0644 "$root/packaging/dome-screencast.portal" "$stage/usr/share/xdg-desktop-portal/portals/dome-screencast.portal"
# Retire DomeD's old Qt-only portal overrides from reused staging trees.
rm -f -- "$stage/etc/systemd/user/xdg-desktop-portal-kde.service.d/50-dome-qt6.conf" \
          "$stage/etc/systemd/user/plasma-xdg-desktop-portal-kde.service.d/50-dome-qt6.conf"
font_runtime="${DOME_FONT_RUNTIME_PREFIX:-${DOME_FLTK_PREFIX:-$root/../fltk-experiment/_toolkit}}"
for library in libpango-1.0.so.0 libpangoft2-1.0.so.0 libpangocairo-1.0.so.0; do
    install -D -m 0644 "$font_runtime/lib/$library" "$stage/usr/local/lib/domed/bitmap/$library"
done
for source in "$font_runtime"/share/pango-bdf-source/*; do
    install -D -m 0644 "$source" "$stage/usr/local/share/doc/domed/pango-bdf/$(basename -- "$source")"
done
install -D -m 0644 "$root/docs/FONT_COLLECTIONS.md" "$stage/usr/local/share/doc/domed/FONT_COLLECTIONS.md"
for name in README.md LICENSE THIRD-PARTY.md dome.example.json; do
    install -D -m 0644 "$root/$name" "$stage/usr/local/share/doc/domed/$name"
done
for name in gtk-shell.xml COPYING.gtk; do
    install -D -m 0644 "$root/protocols/$name" "$stage/usr/local/share/doc/domed/protocols/$name"
done
# These stage subtrees contain only managed distribution artifacts. Avoid
# carrying retired files forward when reusing a previous staging directory.
rm -rf -- "$stage/usr/local/share/fonts/domed" "$stage/usr/local/share/doc/domed/fonts"
while IFS= read -r name; do
    install -D -m 0644 "$root/assets/fonts/$name" "$stage/usr/local/share/doc/domed/fonts/$name"
done < "$root/packaging/font-notices.list"
install -D -m 0644 "$root/scripts/ttf-to-bdf.py" "$stage/usr/local/share/doc/domed/fonts/ttf-to-bdf.py"
while IFS= read -r -d "" font; do
    name="${font#"$root/assets/fonts/"}"
    [[ "$name" == */upstream.bdf ]] && continue
    install -D -m 0644 "$font" "$stage/usr/local/share/fonts/domed/$name"
done < <(find "$root/assets/fonts" -type f -name "*.bdf" -print0)
install -D -m 0644 "$root/assets/fonts/fonts.index" "$stage/usr/local/share/fonts/domed/fonts.index"
desktop-file-validate "$stage/usr/share/wayland-sessions/dome.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"

for page in "$root"/docs/man/*.1; do
    install -D -m 0644 "$page" "$stage/usr/local/share/man/man1/$(basename -- "$page")"
done
/usr/bin/python3 "$root/scripts/stage-link-handlers.py" "$stage"
# Native bundled experiences are part of the default de-xr distribution.
# Minimal compositor-only packagers may opt out explicitly.
if [[ "${DOME_BUNDLED_EXPERIENCES:-1}" != 0 ]]; then
    /usr/bin/python3 "$root/scripts/stage-experiences.py" "$stage"
fi
echo "Staged DomeD installation: $stage"
