#!/usr/bin/env bash
# Optional Fedora/Nobara development setup without installing system packages.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
mkdir -p .local-deps/rpms .local-deps/root
dnf download --arch=x86_64 --destdir .local-deps/rpms qt6-qtbase-devel xorg-x11-server-Xvfb libseat-devel libinput-devel
for package in .local-deps/rpms/*.x86_64.rpm; do
    rpm2cpio "$package" | (cd .local-deps/root && cpio -idm --quiet)
done
ln -sfn /usr/lib64/libseat.so.1 .local-deps/root/usr/lib64/libseat.so
ln -sfn /usr/lib64/libinput.so.10 .local-deps/root/usr/lib64/libinput.so
