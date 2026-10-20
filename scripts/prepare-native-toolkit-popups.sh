#!/usr/bin/env bash
# Build real toolkit fixtures and the FLTK Wayland popup_done compatibility hook.
# --fetch downloads Ubuntu development packages into an isolated local sysroot.
# --install-runtime installs the patched FLTK core for this user's DomeD sessions.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
deps="$root/.local-deps/popup-toolkits"
sysroot="${DOME_TOOLKIT_SYSROOT:-$deps/root}"
fetch=0
install_runtime=0
for arg in "$@"; do
    case "$arg" in
        --fetch) fetch=1 ;;
        --install-runtime) install_runtime=1 ;;
        *) echo "Usage: $0 [--fetch] [--install-runtime]" >&2; exit 2 ;;
    esac
done
mkdir -p "$deps/packages" "$sysroot" "$root/build/toolkit-popups"
if [[ "$fetch" == 1 ]]; then
    (
        cd "$deps/packages"
        apt-get download libfltk1.4-dev libfltk1.4 libsdl3-dev libsdl3-0 \
            libpango1.0-dev libharfbuzz-dev libfribidi-dev libthai-dev libdatrie-dev \
            libsysprof-capture-4-dev libgraphite2-dev libxft-dev wayland-protocols
    )
    for package in "$deps"/packages/*.deb; do dpkg-deb -x "$package" "$sysroot"; done
    python3 - "$sysroot" <<'PY'
from pathlib import Path
import sys
root = Path(sys.argv[1]).resolve()
for path in (root / 'usr').rglob('*.pc'):
    text = path.read_text().replace('prefix=/usr', f'prefix={root}/usr')
    text = text.replace('datadir=/usr/', f'datadir={root}/usr/')
    path.write_text(text)
for path in (root / 'usr/lib/x86_64-linux-gnu').glob('*.so'):
    if path.is_symlink() and not path.exists():
        installed = Path('/usr/lib/x86_64-linux-gnu') / path.readlink()
        if installed.exists():
            path.unlink()
            path.symlink_to(installed)
PY
fi
export PKG_CONFIG_PATH="$sysroot/usr/lib/x86_64-linux-gnu/pkgconfig:$sysroot/usr/share/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
archive="$deps/fltk-1.4.4.tar.gz"
if [[ ! -f "$archive" ]]; then
    curl -fL --retry 2 https://github.com/fltk/fltk/archive/refs/tags/release-1.4.4.tar.gz -o "$archive"
fi
echo "cbf5f7846af596206e8e4489e14c9981f98d7b37168110a00dcd26d8d479a669  $archive" | sha256sum --check --status
source_dir="$deps/fltk-domed-source"
if [[ ! -d "$source_dir" ]]; then
    mkdir -p "$source_dir"
    tar -xf "$archive" -C "$source_dir" --strip-components=1
    patch -d "$source_dir" -p1 < "$root/native/toolkit-hooks/fltk-1.4.4-popup-done.patch"
else
    patch --reverse --dry-run --silent -d "$source_dir" -p1 < "$root/native/toolkit-hooks/fltk-1.4.4-popup-done.patch"
fi
build="$root/build/fltk-domed-runtime"
cmake -S "$source_dir" -B "$build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_SKIP_RPATH=ON -DCMAKE_PREFIX_PATH="$sysroot/usr" \
    -DFLTK_BACKEND_WAYLAND=ON -DFLTK_BACKEND_X11=OFF -DFLTK_BUILD_SHARED_LIBS=ON \
    -DFLTK_BUILD_TEST=OFF -DFLTK_BUILD_EXAMPLES=OFF -DFLTK_BUILD_FLUID=OFF \
    -DFLTK_BUILD_FLTK_OPTIONS=OFF -DFLTK_BUILD_FORMS=OFF -DFLTK_BUILD_GL=OFF \
    -DFLTK_USE_DBUS=OFF -DFLTK_OPTION_CAIRO_WINDOW=ON
cmake --build "$build" --target fltk-shared --parallel "${DOME_BUILD_JOBS:-4}"
if ! rg -q '^#define FLTK_USE_WAYLAND 1' "$build/FL/fl_config.h"; then
    echo 'FLTK was built without its native Wayland backend.' >&2; exit 1
fi
g++ -std=c++17 -Wall -Wextra "$root/scripts/fltk-popup-client.cpp" \
    $(pkg-config --cflags cairo) -I"$sysroot/usr/include" \
    -L"$sysroot/usr/lib/x86_64-linux-gnu" -Wl,-rpath,"$sysroot/usr/lib/x86_64-linux-gnu" \
    -lfltk -o "$root/build/toolkit-popups/fltk-popup-client"
if [[ -f "$sysroot/usr/include/SDL3/SDL.h" ]]; then
    # Dynamic linking only needs the public headers/core. SDL's .pc also lists
    # optional backend development dependencies which this fixture does not use.
    g++ -std=c++17 -Wall -Wextra "$root/scripts/sdl-popup-client.cpp" \
        -I"$sysroot/usr/include" -L"$sysroot/usr/lib/x86_64-linux-gnu" \
        -Wl,-rpath,"$sysroot/usr/lib/x86_64-linux-gnu" -lSDL3 \
        -o "$root/build/toolkit-popups/sdl-popup-client"
else
    g++ -std=c++17 -Wall -Wextra "$root/scripts/sdl-popup-client.cpp" \
        $(pkg-config --cflags --libs sdl3) -o "$root/build/toolkit-popups/sdl-popup-client"
fi
if [[ "$install_runtime" == 1 ]]; then
    destination="${DOME_FLTK_INSTALL_DIR:-$HOME/.local/lib/domed/fltk}"
    install -D -m 0755 "$build/lib/libfltk.so.1.4.4" "$destination/libfltk.so.1.4"
    documentation="${XDG_DATA_HOME:-$HOME/.local/share}/domed/toolkit-hooks/fltk"
    install -D -m 0644 "$source_dir/COPYING" "$documentation/COPYING"
    install -D -m 0644 "$archive" "$documentation/fltk-1.4.4.tar.gz"
    install -D -m 0644 "$root/native/toolkit-hooks/fltk-1.4.4-popup-done.patch" "$documentation/popup-done.patch"
    install -D -m 0644 "$root/native/toolkit-hooks/README.md" "$documentation/README.md"
    echo "Installed DomeD's native Wayland FLTK compatibility runtime: $destination"
fi
