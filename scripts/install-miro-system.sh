#!/usr/bin/env bash
# Install a prepared native Miro build. Compilation always runs as the user.
set -euo pipefail
# Public parent directories must remain readable even with a private sudo umask.
umask 022
[[ $(id -u) -eq 0 ]] || { echo 'Run with sudo -A bash scripts/install-miro-system.sh STAGE [DESKTOP_DATABASE_TOOL]' >&2; exit 1; }
stage=${1:?Prepared DESTDIR stage required}
desktop_database_tool=${2:-/usr/bin/update-desktop-database}
files=(
    usr/local/share/fonts/miro-pdf/unifont-17.0.04.bdf
    usr/local/share/licenses/miro-pdf/COPYING.unifont
    usr/local/share/licenses/miro-pdf/LICENSE
    usr/local/share/licenses/miro-pdf/LICENSE.MIT
    usr/local/share/licenses/miro-pdf/UNLICENSE
    usr/local/share/doc/miro-pdf/NOTICE
    usr/local/share/doc/miro-pdf/UPSTREAM.json
    usr/local/share/doc/miro-pdf/VALIDATION.md
    usr/local/share/applications/miro-pdf.desktop
    usr/local/share/icons/hicolor/256x256/apps/miro-pdf.png
    usr/local/share/miro-pdf/default.conf
    usr/local/bin/miro-pdf
)
for file in "${files[@]}"; do
    [[ -f "$stage/$file" && ! -L "$stage/$file" ]] || { echo "Missing prepared file: $file" >&2; exit 1; }
done
exec 9>/run/lock/domed-install.lock
flock -x 9
backup="/var/backups/domed/miro-$(date -u +%Y%m%d-%H%M%S)"
install -d -o root -g root -m0755 "$backup"
for file in "${files[@]}"; do
    if [[ -e /$file || -L /$file ]]; then
        install -d -m0755 "$backup/$(dirname -- "$file")"
        cp -a -- "/$file" "$backup/$file"
    fi
    mode=0644
    [[ "$file" != usr/local/bin/* ]] || mode=0755
    install -d -o root -g root -m0755 "$(dirname -- "/$file")"
    temporary="/$file.miro-install-$$"
    install -o root -g root -m "$mode" "$stage/$file" "$temporary"
    mv -fT -- "$temporary" "/$file"
done
(cd /; sha256sum -- "${files[@]}") > /usr/local/share/miro-pdf/install.sha256
chmod 0644 /usr/local/share/miro-pdf/install.sha256
cp /usr/local/share/miro-pdf/install.sha256 "$backup/installed.sha256"
FONTCONFIG_FILE=/etc/fonts/fonts.conf fc-cache -f /usr/local/share/fonts/miro-pdf
if [[ -x "$desktop_database_tool" ]]; then
    "$desktop_database_tool" /usr/local/share/applications
fi
if command -v restorecon >/dev/null; then
    restorecon /usr/local/bin/miro-pdf
fi
printf 'Installed native FLTK Miro system-wide: /usr/local/bin/miro-pdf\nBackup and file manifest: %s\n' "$backup"
