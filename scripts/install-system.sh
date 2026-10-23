#!/usr/bin/env bash
# Install only a prepared build. Never compile as root.
set -euo pipefail
[[ $(id -u) -eq 0 ]] || { echo "Run with pkexec /usr/bin/bash $0" >&2; exit 1; }
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="${1:-$root/build/system-stage}"
# Keep concurrent prepared installs from interleaving backups and checksums.
exec 9>/run/lock/domed-install.lock
/usr/bin/flock -x 9
files=(
    etc/systemd/user/dome-session.target
    etc/systemd/user/plasma-xdg-desktop-portal-kde.service.d/50-dome-qt6.conf
    etc/systemd/user/xdg-desktop-portal-kde.service.d/50-dome-qt6.conf
    usr/local/share/man/man1/dome-askpass.1
    usr/local/share/man/man1/dome-compositor.1
    usr/local/share/man/man1/dome-xr.1
    usr/local/share/man/man1/dome-preview.1
    usr/local/bin/dome-compositor
    usr/local/bin/dome-xr
    usr/local/bin/dome-preview
    usr/local/bin/dome-portal-picker
    usr/local/bin/dome-bdf-font
    usr/local/bin/dome-session
    usr/local/share/doc/domed/TRAY_FRONTENDS.md
    usr/local/share/doc/domed/SCREENSHOTS.md
    usr/local/share/doc/domed/FLTK_GPU_AUDIT.md
    usr/local/bin/dome-tray
    usr/local/bin/fcitx5-configuration-fltk
    usr/local/share/applications/org.domed.Tray.desktop
    usr/local/share/icons/hicolor/scalable/apps/org.domed.Tray.svg
    usr/local/share/applications/org.domed.Connect.desktop
    usr/local/share/applications/org.domed.InputMethods.desktop
    usr/local/share/applications/org.domed.Wallets.desktop
    usr/local/bin/dome-askpass
    usr/local/libexec/domed/pinentry-fltk
    usr/local/libexec/dome-session-client
    usr/share/xdg-desktop-portal/dome-portals.conf
    usr/share/xdg-desktop-portal/portals/dome-settings.portal
    usr/share/xdg-desktop-portal/portals/dome-screencast.portal
    usr/share/wayland-sessions/dome.desktop
    usr/local/share/applications/org.domed.Compositor.desktop
    usr/local/share/doc/domed/README.md
    usr/local/share/doc/domed/LICENSE
    usr/local/share/doc/domed/THIRD-PARTY.md
    usr/local/share/doc/domed/fonts/ttf-to-bdf.py
    usr/local/share/doc/domed/dome.example.json
    usr/local/share/doc/domed/protocols/gtk-shell.xml
    usr/local/share/doc/domed/protocols/COPYING.gtk
)
for name in COPYING source.sha256 pinentry-1.3.3.tar.bz2 dome_pinentry.h pinentry-1.3.3-dome.patch session_environment.h fltk_appearance.h bdf_font.hpp build-askpass.py; do
    files+=("usr/local/share/doc/domed/pinentry/$name")
done
while IFS= read -r name; do
    files+=("usr/local/share/doc/domed/fonts/$name")
done < "$root/packaging/font-notices.list"
while IFS= read -r -d "" file; do
    files+=("${file#"$stage/"}")
done < <(find "$stage/usr/local/share/fonts/domed" "$stage/usr/local/lib/domed/bitmap" "$stage/usr/local/share/doc/domed/pango-bdf" -type f -print0)
for file in "${files[@]}"; do
    [[ -f "$stage/$file" && ! -L "$stage/$file" ]] || { echo "Missing staged file: $file" >&2; exit 1; }
done
[[ -x /usr/local/bin/flterm-xr-launch || -x /usr/local/bin/ghostty-xr-launch ]] || { echo 'Install FLTerm XR or Ghostty XR first.' >&2; exit 1; }
/usr/bin/desktop-file-validate "$stage/usr/share/wayland-sessions/dome.desktop" "$stage/usr/local/share/applications/org.domed.Compositor.desktop"
backup="/var/backups/domed/$(date +%Y%m%d-%H%M%S)"
for file in "${files[@]}"; do
    target="/$file"
    if [[ -e "$target" || -L "$target" ]]; then
        /usr/bin/install -d -m 0755 "$backup/$(dirname -- "$file")"
        /usr/bin/cp -a -- "$target" "$backup/$file"
    fi
    mode=0644
    [[ "$file" == usr/local/bin/* || "$file" == usr/local/libexec/* ]] && mode=0755
    # A prior install can leave a documentation directory with the caller's
    # restrictive umask. Installed public files must remain readable by users.
    /usr/bin/install -d -o root -g root -m 0755 "$(dirname -- "$target")"
    /usr/bin/install -D -o root -g root -m "$mode" -- "$stage/$file" "$target"
done
/usr/bin/python3 "$root/scripts/prune-retired-fonts.py" "$root/packaging/retired-font-files.json" "$backup"
/usr/bin/install -d -m 0755 /usr/local/share/domed
cd /
sha256sum -- "${files[@]}" > /usr/local/share/domed/install.sha256
chmod 0644 /usr/local/share/domed/install.sha256
if command -v restorecon >/dev/null; then
    restorecon /usr/share/wayland-sessions/dome.desktop /usr/local/bin/dome-* /usr/local/bin/fcitx5-configuration-fltk /usr/local/libexec/dome-session-client
fi
if command -v update-desktop-database >/dev/null; then update-desktop-database /usr/local/share/applications; fi
echo 'Installed DomeD in /usr/local and registered the DomeD login session.'
[[ ! -d "$backup" ]] || echo "Previous installation backup: $backup"

# All base files are installed; release their lock before the translator's
# verified installer acquires the same lock for its own backup transaction.
if [[ -f "$stage/translator-manifest.json" ]]; then
    flock -u 9
    /usr/bin/python3 "$root/scripts/install-translator.py" "$stage"
fi
