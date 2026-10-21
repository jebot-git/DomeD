#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
build_args=()
features=nested,xr,standalone
while [[ $# -gt 0 ]]; do
    case "$1" in
        --release) build_args=(--release); shift ;;
        --multi-gpu) features+=,multi-gpu; shift ;;
        --drm-device)
            [[ $# -ge 2 && "$2" == /* ]] || { echo '--drm-device requires an absolute DRM device path' >&2; exit 2; }
            export DOME_BUILD_DRM_DEVICE="$2"
            shift 2 ;;
        *) echo 'Usage: build-local.sh [--release] [--multi-gpu | --drm-device /dev/dri/by-path/...-card]' >&2; exit 2 ;;
    esac
done
if [[ "$features" == *,multi-gpu && -n "${DOME_BUILD_DRM_DEVICE:-}" ]]; then
    echo '--multi-gpu and DOME_BUILD_DRM_DEVICE/--drm-device are mutually exclusive' >&2
    exit 2
fi
# Optional locally extracted Fedora Qt development RPM; host runtime is used.
if [[ -d "$root/.local-deps/root/usr/include/qt6" ]]; then
    export DOME_QT6_PREFIX="$root/.local-deps/root/usr"
    export DOME_QT6_LIBDIR=/usr/lib64
fi
export LIBRARY_PATH="$root/.local-deps/root/usr/lib64${LIBRARY_PATH:+:$LIBRARY_PATH}"
export PKG_CONFIG_PATH="$root/.local-deps/root/usr/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
cargo build --locked --features "$features" "${build_args[@]}"
cargo test --locked --features "$features"
