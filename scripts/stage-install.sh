#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="${DOME_STAGE_DIR:-$root/build/system-stage}"
release="${DOME_RELEASE_DIR:-$root/target/release}"
"$release/dome-compositor" --backend drm --check-backend
for name in dome-compositor dome-xr dome-preview dome-portal-picker; do
    install -D -m 0755 "$release/$name" "$stage/usr/local/bin/$name"
done
install -D -m 0755 "$root/packaging/dome-session" "$stage/usr/local/bin/dome-session"
install -D -m 0755 "$root/packaging/dome-session-client" "$stage/usr/local/libexec/dome-session-client"
install -D -m 0644 "$root/packaging/dome.desktop" "$stage/usr/share/wayland-sessions/dome.desktop"
install -D -m 0644 "$root/packaging/org.domed.Compositor.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"
install -D -m 0644 "$root/packaging/dome-portals.conf" "$stage/usr/share/xdg-desktop-portal/dome-portals.conf"
install -D -m 0644 "$root/packaging/dome-settings.portal" "$stage/usr/share/xdg-desktop-portal/portals/dome-settings.portal"
install -D -m 0644 "$root/packaging/dome-screencast.portal" "$stage/usr/share/xdg-desktop-portal/portals/dome-screencast.portal"
for unit in plasma-xdg-desktop-portal-kde xdg-desktop-portal-kde; do
    install -D -m 0644 "$root/packaging/dome-portal-qt6.conf" "$stage/etc/systemd/user/$unit.service.d/50-dome-qt6.conf"
done
for name in README.md LICENSE THIRD-PARTY.md dome.example.json; do
    install -D -m 0644 "$root/$name" "$stage/usr/local/share/doc/domed/$name"
done
for name in gtk-shell.xml COPYING.gtk; do
    install -D -m 0644 "$root/protocols/$name" "$stage/usr/local/share/doc/domed/protocols/$name"
done
while IFS= read -r name; do
    install -D -m 0644 "$root/assets/fonts/$name" "$stage/usr/local/share/doc/domed/fonts/$name"
done < "$root/packaging/font-notices.list"
desktop-file-validate "$stage/usr/share/wayland-sessions/dome.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"

for page in "$root"/docs/man/*.1; do
    install -D -m 0644 "$page" "$stage/usr/local/share/man/man1/$(basename -- "$page")"
done
echo "Staged DomeD installation: $stage"
