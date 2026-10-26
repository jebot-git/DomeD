#!/usr/bin/env bash
# Install prepared tray frontends and bridge placement without replacing the suite launcher.
set -euo pipefail
[[ $(id -u) -eq 0 ]] || { echo 'Run with sudo -A bash scripts/install-tray.sh STAGE [COMPOSITOR_TARGET]' >&2; exit 1; }
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="${1:?Prepared stage directory required}"
compositor="${2:-/usr/local/bin/dome-compositor}"
case "$compositor" in
    /usr/local/bin/dome-compositor|/opt/fltk-vr/bin/dome-compositor) ;;
    *) echo 'Unsupported compositor installation path' >&2; exit 1 ;;
esac
exec 9>/run/lock/domed-install.lock
flock -x 9
files=(usr/local/bin/dome-tray usr/local/bin/dome-session
    usr/local/bin/dome-backup-job
    usr/local/bin/fcitx5-configuration-fltk
    usr/local/share/applications/org.domed.Tray.desktop
    usr/local/share/icons/hicolor/scalable/apps/org.domed.Tray.svg
    usr/local/share/icons/hicolor/scalable/apps/org.domed.Network.svg
    usr/local/share/icons/hicolor/scalable/apps/org.domed.Backups.svg
    usr/local/share/applications/org.domed.Network.desktop
    usr/local/share/applications/org.domed.Backups.desktop
    usr/local/share/applications/org.domed.Connect.desktop
    usr/local/share/applications/org.domed.InputMethods.desktop
    usr/local/share/applications/org.domed.Bluetooth.desktop
    usr/local/share/applications/org.domed.Wallets.desktop
    usr/local/share/doc/domed/BLUETOOTH.md
    usr/local/share/doc/domed/TRAY_FRONTENDS.md
    usr/local/share/doc/domed/NETWORK_BACKUPS_MIXER.md
    usr/local/share/doc/domed/SCREENSHOTS.md
    usr/local/share/doc/domed/FLTK_GPU_AUDIT.md)
files+=("${compositor#/}")
for file in "${files[@]}"; do
    [[ -f "$stage/$file" && ! -L "$stage/$file" ]] || { echo "Missing staged file: $file" >&2; exit 1; }
done
backup="/var/backups/domed/tray-$(date +%Y%m%d-%H%M%S)"
for file in "${files[@]}"; do
    if [[ -e /$file || -L /$file ]]; then
        install -d -m 0755 "$backup/$(dirname -- "$file")"
        cp -a -- "/$file" "$backup/$file"
    fi
    mode=0644
    case "$file" in usr/local/bin/*|usr/local/libexec/*|opt/fltk-vr/bin/*) mode=0755 ;; esac
    install -d -o root -g root -m 0755 "$(dirname -- "/$file")"
    install -o root -g root -m "$mode" "$stage/$file" "/$file"
done
install -d -m 0755 /usr/local/share/domed
(cd /; sha256sum -- "${files[@]}") > /usr/local/share/domed/tray-install.sha256
chmod 0644 /usr/local/share/domed/tray-install.sha256
if command -v restorecon >/dev/null; then
    restorecon /usr/local/bin/dome-tray /usr/local/bin/dome-backup-job /usr/local/bin/fcitx5-configuration-fltk /usr/local/bin/dome-session "$compositor"
fi
if command -v update-desktop-database >/dev/null; then update-desktop-database /usr/local/share/applications; fi
echo "Installed native FLTK tray managers, backup helper and compositor; backup: $backup"
echo 'New DomeD sessions use the native managers. Existing desktop sessions were left running.'

# All base files are installed; release their lock before the translator's
# verified installer acquires the same lock for its own backup transaction.
if [[ -f "$stage/translator-manifest.json" ]]; then
    flock -u 9
    /usr/bin/python3 "$root/scripts/install-translator.py" "$stage"
fi
