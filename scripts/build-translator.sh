#!/usr/bin/env bash
# Build as the login user, against the installed FLTK toolkit.
set -euo pipefail
[[ $(id -u) -ne 0 ]] || { echo 'Build translator as a regular user.' >&2; exit 1; }
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
build="${DOME_TRANSLATOR_BUILD_DIR:-$root/build/translator}"
prefix="${DOME_TRANSLATOR_PREFIX:-/usr/local}"
cmake -S "$root/native/translator" -B "$build" -G Ninja \
    -DCMAKE_PREFIX_PATH="${DOME_FLTK_PREFIX:-/opt/fltk-vr/toolkit}" \
    -DFLTK_DIR="${DOME_FLTK_PREFIX:-/opt/fltk-vr/toolkit}/share/fltk" \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_INTERPROCEDURAL_OPTIMIZATION=ON \
    -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_INSTALL_LIBEXECDIR=libexec
rm -rf -- "$build/vendor-next"
python3 -m pip install --only-binary=:all: --target "$build/vendor-next" \
    -r "$root/native/translator/requirements.txt"
rm -rf -- "$build/vendor"
mv -- "$build/vendor-next" "$build/vendor"
cmake --build "$build" --parallel "${DOME_BUILD_JOBS:-6}"
ctest --test-dir "$build" --output-on-failure
