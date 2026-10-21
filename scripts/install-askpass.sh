#!/usr/bin/env bash
# Install prepared authentication integration without replacing the suite launcher.
set -euo pipefail
[[ $(id -u) -eq 0 ]] || { echo 'Run with sudo -A bash scripts/install-askpass.sh STAGE [COMPOSITOR_TARGET]' >&2; exit 1; }
stage="${1:?Prepared stage directory required}"
compositor="${2:-/usr/local/bin/dome-compositor}"
case "$compositor" in
    /usr/local/bin/dome-compositor|/opt/fltk-vr/bin/dome-compositor) ;;
    *) echo 'Unsupported compositor installation path' >&2; exit 1 ;;
esac
exec 9>/run/lock/domed-install.lock
flock -x 9
files=(usr/local/bin/dome-askpass usr/local/libexec/domed/pinentry-fltk usr/local/bin/dome-session usr/local/share/man/man1/dome-askpass.1)
for name in COPYING source.sha256 pinentry-1.3.3.tar.bz2 dome_pinentry.h pinentry-1.3.3-dome.patch fltk_appearance.h build-askpass.py; do
    files+=("usr/local/share/doc/domed/pinentry/$name")
done
files+=("${compositor#/}")
for file in "${files[@]}"; do
    [[ -f "$stage/$file" && ! -L "$stage/$file" ]] || { echo "Missing staged file: $file" >&2; exit 1; }
done
backup="/var/backups/domed/askpass-$(date +%Y%m%d-%H%M%S)"
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
(cd /; sha256sum -- "${files[@]}") > /usr/local/share/domed/askpass-install.sha256
chmod 0644 /usr/local/share/domed/askpass-install.sha256
if command -v restorecon >/dev/null; then
    restorecon /usr/local/bin/dome-askpass /usr/local/libexec/domed/pinentry-fltk /usr/local/bin/dome-session "$compositor"
fi
echo "Installed native FLTK authentication; backup: $backup"
echo 'New DomeD processes use the helper. Existing desktop sessions were left running.'
