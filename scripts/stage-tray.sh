#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="${1:-${DOME_STAGE_DIR:-$root/build/system-stage}}"
install -D -m 0755 "${DOME_TRAY_BUILD_DIR:-$root/build/tray}/dome-tray" "$stage/usr/local/bin/dome-tray"
install -D -m 0755 "${DOME_TRAY_BUILD_DIR:-$root/build/tray}/fcitx5-configuration-fltk" "$stage/usr/local/bin/fcitx5-configuration-fltk"
install -D -m 0755 "$root/packaging/dome-session" "$stage/usr/local/bin/dome-session"
for doc in TRAY_FRONTENDS.md FLTK_GPU_AUDIT.md SCREENSHOTS.md BLUETOOTH.md; do
    install -D -m 0644 "$root/docs/$doc" "$stage/usr/local/share/doc/domed/$doc"
done
for app in Tray Connect InputMethods Wallets Bluetooth; do
    install -D -m 0644 "$root/packaging/org.domed.$app.desktop" "$stage/usr/local/share/applications/org.domed.$app.desktop"
    desktop-file-validate "$stage/usr/local/share/applications/org.domed.$app.desktop"
done

install -D -m 0644 "$root/native/icons/tray-applications.svg" "$stage/usr/local/share/icons/hicolor/scalable/apps/org.domed.Tray.svg"

if [[ "${DOME_BUILD_TRANSLATOR:-1}" != 0 ]]; then
    "$root/scripts/stage-translator.sh" "$stage"
fi
