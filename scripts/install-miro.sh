#!/usr/bin/env bash
# Build the native FLTK Miro fork with DomeD's bitmap fonts and universal menu.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
revision=c264d88ee969910a397ec8511f191c0aa4477c7b
repository=https://github.com/jebot-git/miro.git
source_dir=${DOME_MIRO_SOURCE:-"$root/../miro"}
worktree="$root/build/miro-fltk-source"
build_dir=${DOME_MIRO_BUILD:-"$root/build/miro-fltk-build"}
prefix=${DOME_MIRO_PREFIX:-"$HOME/.local"}
if [[ ! -d "$source_dir/.git" && ! -f "$source_dir/.git" ]]; then
    if [[ -e "$source_dir" ]]; then
        printf 'Miro source already exists without Git metadata: %s\n' "$source_dir" >&2
        exit 1
    fi
    git clone "$repository" "$source_dir"
fi
# Fetch this fork explicitly, so an existing vanilla-upstream checkout is safe.
git -C "$source_dir" fetch "$repository" "$revision"
if [[ ! -d "$worktree" ]]; then
    git -C "$source_dir" worktree add --detach "$worktree" "$revision"
elif [[ $(git -C "$worktree" rev-parse HEAD) != "$revision" ]]; then
    if [[ -n $(git -C "$worktree" status --porcelain) ]]; then
        printf 'Preserving modified Miro worktree: %s\n' "$worktree" >&2
        exit 1
    fi
    git -C "$worktree" checkout --detach "$revision"
fi
export MIRO_BUILD_DIR="$build_dir"
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$source_dir/target"}
"$worktree/scripts/build.sh"
ctest --test-dir "$build_dir" --output-on-failure
# Preserve the prior optional viewer before replacing an existing installation.
if [[ -f "$prefix/bin/miro-pdf" ]]; then
    backup="$prefix/share/miro-pdf/backups/$(date -u +%Y%m%d-%H%M%S)"
    mkdir -p "$backup"
    cp -p "$prefix/bin/miro-pdf" "$backup/miro-pdf"
    if [[ -f "$prefix/share/applications/miro-pdf.desktop" ]]; then
        cp -p "$prefix/share/applications/miro-pdf.desktop" "$backup/miro-pdf.desktop"
    fi
    printf 'Previous Miro preserved in %s\n' "$backup"
fi
cmake --install "$build_dir" --prefix "$prefix"
fc-cache -f "$prefix/share/fonts"
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$prefix/share/applications"
fi
printf 'Installed native FLTK Miro (%s): %s/bin/miro-pdf\n' "$revision" "$prefix"
