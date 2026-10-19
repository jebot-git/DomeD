#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
build_args=()
case "${1:-}" in
    '') ;;
    --release) build_args=(--release) ;;
    *) echo 'Usage: build-local.sh [--release]' >&2; exit 2 ;;
esac
# Optional locally extracted Fedora Qt development RPM; host runtime is used.
if [[ -d "$root/.local-deps/root/usr/include/qt6" ]]; then
    export DOME_QT6_PREFIX="$root/.local-deps/root/usr"
    export DOME_QT6_LIBDIR=/usr/lib64
fi
export LIBRARY_PATH="$root/.local-deps/root/usr/lib64${LIBRARY_PATH:+:$LIBRARY_PATH}"
export PKG_CONFIG_PATH="$root/.local-deps/root/usr/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo build --locked --features nested,xr,standalone "${build_args[@]}"
cargo test --locked --features nested,xr,standalone
