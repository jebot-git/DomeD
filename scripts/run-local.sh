#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -z "${DOME_TERMINAL:-}" ]]; then
    if [[ "${DOME_TERMINAL_MODE:-overlay}" == wayland ]]; then
        export DOME_TERMINAL="$root/../ghostty-xr/build/ghostty-xr"
    elif [[ -x /usr/local/bin/ghostty-xr-launch ]]; then
        export DOME_TERMINAL=/usr/local/bin/ghostty-xr-launch
    else
        export DOME_TERMINAL="$root/../ghostty-xr/build/ghostty-xr"
    fi
fi
if [[ ! -x "$DOME_TERMINAL" ]]; then
    echo "Build Ghostty XR first, or set DOME_TERMINAL to its executable." >&2
    exit 1
fi
if [[ $# -eq 0 ]]; then set -- --terminal; fi
exec "$root/target/debug/dome-compositor" "$@"
