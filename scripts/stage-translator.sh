#!/usr/bin/env bash
# Prepare a self-contained system install and checksummed file manifest.
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
build="${DOME_TRANSLATOR_BUILD_DIR:-$root/build/translator}"
stage="${1:-$root/build/translator-system-stage}"
DESTDIR="$stage" cmake --install "$build" --prefix /usr/local
python3 - "$stage" "$build/install_manifest.txt" <<'PY'
import hashlib,json,sys
from pathlib import Path
stage=Path(sys.argv[1])
files=[stage/Path(line).relative_to('/') for line in Path(sys.argv[2]).read_text().splitlines()]
manifest={str(p.relative_to(stage)):{'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),
    'mode':0o755 if p.name=='dome-translator' else 0o644} for p in files if p.is_file()}
(stage/'translator-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print('Staged',len(manifest),'files; manifest:',stage/'translator-manifest.json')
PY
