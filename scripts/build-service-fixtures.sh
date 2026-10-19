#!/usr/bin/env bash
# Build the KDE Connect clipboard probe without installing development packages.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p .local-deps/rpms .local-deps/root build
dnf download --arch=x86_64 --destdir .local-deps/rpms wl-clipboard kf6-kguiaddons-devel
for package in .local-deps/rpms/wl-clipboard-*.rpm .local-deps/rpms/kf6-kguiaddons-devel-*.rpm; do
    rpm2cpio "$package" | (cd .local-deps/root && cpio -idm --quiet)
done
g++ -std=c++17 -fPIC -I.local-deps/root/usr/include/qt6 -I.local-deps/root/usr/include/qt6/QtCore -I.local-deps/root/usr/include/qt6/QtGui -I.local-deps/root/usr/include/KF6/KGuiAddons scripts/kde-clipboard-client.cpp -l:libQt6Gui.so.6 -l:libQt6Core.so.6 -l:libKF6GuiAddons.so.6 -o build/kde-clipboard-client
