#!/usr/bin/env bash
# Configure and build native service frontends as the regular user.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cmake -S "$root/native/tray" -B "${DOME_TRAY_BUILD_DIR:-$root/build/tray}" -G Ninja \
    -DCMAKE_PREFIX_PATH="${DOME_FLTK_PREFIX:-/opt/fltk-vr/toolkit}" \
    -DFLTK_DIR="${DOME_FLTK_PREFIX:-/opt/fltk-vr/toolkit}/share/fltk" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_CXX_FLAGS_RELEASE='-O3 -DNDEBUG' \
    -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON
cmake --build "${DOME_TRAY_BUILD_DIR:-$root/build/tray}" --parallel "${DOME_BUILD_JOBS:-6}"
