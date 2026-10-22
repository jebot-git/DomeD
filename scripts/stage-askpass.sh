#!/usr/bin/env bash
# Prepare the native helper and its corresponding source, without root.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
stage="${1:-${DOME_STAGE_DIR:-$root/build/system-stage}}"
build="${DOME_ASKPASS_BUILD_DIR:-$root/build/askpass}"
install -D -m 0755 "$build/fltk/pinentry-fltk" "$stage/usr/local/libexec/domed/pinentry-fltk"
install -D -m 0755 "$build/dome-askpass" "$stage/usr/local/bin/dome-askpass"
install -D -m 0755 "$root/packaging/dome-session" "$stage/usr/local/bin/dome-session"
install -D -m 0644 "$root/docs/man/dome-askpass.1" "$stage/usr/local/share/man/man1/dome-askpass.1"
documentation="$stage/usr/local/share/doc/domed/pinentry"
install -D -m 0644 "$build/COPYING.pinentry" "$documentation/COPYING"
install -D -m 0644 "$build/source.sha256" "$documentation/source.sha256"
install -D -m 0644 "$build/pinentry-1.3.3.tar.bz2" "$documentation/pinentry-1.3.3.tar.bz2"
for name in dome_pinentry.h session_environment.h pinentry-1.3.3-dome.patch; do
    install -D -m 0644 "$root/native/pinentry/$name" "$documentation/$name"
done
install -D -m 0644 "$root/native/fltk_appearance.h" "$documentation/fltk_appearance.h"
install -D -m 0644 "$root/scripts/build-askpass.py" "$documentation/build-askpass.py"
echo "Staged native FLTK askpass: $stage/usr/local/bin/dome-askpass"
