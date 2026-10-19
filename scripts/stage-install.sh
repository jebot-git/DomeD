#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="$root/build/system-stage"
"$root/target/release/dome-compositor" --backend drm --check-backend
for name in dome-compositor dome-xr dome-preview; do
    install -D -m 0755 "$root/target/release/$name" "$stage/usr/local/bin/$name"
done
install -D -m 0755 "$root/packaging/dome-session" "$stage/usr/local/bin/dome-session"
install -D -m 0755 "$root/packaging/dome-session-client" "$stage/usr/local/libexec/dome-session-client"
install -D -m 0644 "$root/packaging/dome.desktop" "$stage/usr/share/wayland-sessions/dome.desktop"
install -D -m 0644 "$root/packaging/org.domed.Compositor.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"
install -D -m 0644 "$root/packaging/dome-portals.conf" "$stage/usr/share/xdg-desktop-portal/dome-portals.conf"
for name in README.md LICENSE THIRD-PARTY.md dome.example.json; do
    install -D -m 0644 "$root/$name" "$stage/usr/local/share/doc/domed/$name"
done
desktop-file-validate "$stage/usr/share/wayland-sessions/dome.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"
echo "Staged DomeD installation: $stage"
