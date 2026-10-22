#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ -z "${DOME_TERMINAL:-}" ]]; then
    if [[ "${DOME_TERMINAL_BACKEND:-auto}" != ghostty && -x "$root/../flterm-xr/build/flterm-xr" ]]; then
        export DOME_TERMINAL="$root/../flterm-xr/build/flterm-xr"
    elif [[ "${DOME_TERMINAL_BACKEND:-auto}" != ghostty && -x /usr/local/bin/flterm-xr ]]; then
        export DOME_TERMINAL=/usr/local/bin/flterm-xr
    elif [[ "${DOME_TERMINAL_BACKEND:-auto}" == flterm ]]; then
        echo "Build FLTerm XR first, or set DOME_TERMINAL to its executable." >&2
        exit 1
    elif [[ "${DOME_TERMINAL_MODE:-overlay}" == wayland ]]; then
        export DOME_TERMINAL="$root/../ghostty-xr/build/ghostty-xr"
    elif [[ -x /usr/local/bin/ghostty-xr-launch ]]; then
        export DOME_TERMINAL=/usr/local/bin/ghostty-xr-launch
    else
        export DOME_TERMINAL="$root/../ghostty-xr/build/ghostty-xr"
    fi
fi
if [[ ! -x "$DOME_TERMINAL" ]]; then
    echo "Build FLTerm XR or Ghostty XR first, or set DOME_TERMINAL to its executable." >&2
    exit 1
fi
if [[ $# -eq 0 ]]; then set -- --terminal; fi
exec "$root/target/debug/dome-compositor" "$@"
