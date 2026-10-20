#!/usr/bin/env bash
# Build the recommended PDF viewer with Dome's desktop-theme integration.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
revision=d35322c47d53903638558162a5e3153f32dfdc37
source_dir=${DOME_MIRO_SOURCE:-"$HOME/miro"}
worktree="$root/build/miro-source"
prefix=${DOME_MIRO_PREFIX:-"$HOME/.local"}
if [[ ! -d "$source_dir/.git" ]]; then
    git clone https://github.com/vincent-uden/miro.git "$source_dir"
fi
git -C "$source_dir" fetch origin
# Keep the verified patch pinned; updating upstream requires rebasing it first.
if [[ ! -d "$worktree" ]]; then
    git -C "$source_dir" worktree add --detach "$worktree" "$revision"
fi
[[ $(git -C "$worktree" rev-parse HEAD) == "$revision" ]]
patch="$root/native/toolkit-hooks/miro-0.12.0-dome-theme.patch"
if git -C "$worktree" apply --check "$patch" 2>/dev/null; then
    git -C "$worktree" apply "$patch"
else
    git -C "$worktree" apply --reverse --check "$patch"
fi
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$source_dir/target"}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-2}
export CARGO_PROFILE_RELEASE_LTO=false
export CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cd "$worktree"
cargo check --locked --release
cargo build --locked --release
cargo test --locked --release
install -Dm755 "$CARGO_TARGET_DIR/release/miro-pdf" "$prefix/bin/miro-pdf"
install -Dm644 assets/logo.png "$prefix/share/pixmaps/miro-pdf.png"
install -Dm644 LICENSE "$prefix/share/miro-pdf/LICENSE"
install -Dm644 "$patch" "$prefix/share/miro-pdf/dome-theme.patch"
printf '%s\n' "https://github.com/vincent-uden/miro" "$revision" > "$prefix/share/miro-pdf/upstream.txt"
mkdir -p "$prefix/share/applications"
# Desktop Exec values require quoted paths (including installations with spaces).
binary="$prefix/bin/miro-pdf"
escaped=${binary//\\/\\\\}
escaped=${escaped//\"/\\\"}
escaped=${escaped//\$/\\\$}
escaped=${escaped//\`/\\\`}
escaped=${escaped//%/%%}
cat > "$prefix/share/applications/miro-pdf.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Miro PDF
Comment=Read PDF documents
Exec="$escaped" %f
Icon=$prefix/share/pixmaps/miro-pdf.png
Terminal=false
Categories=Office;Viewer;
MimeType=application/pdf;
StartupWMClass=miro-pdf
EOF
update-desktop-database "$prefix/share/applications"
printf 'Installed Miro 0.12.0 with Dome appearance support: %s\n' "$binary"
