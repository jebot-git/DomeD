#!/usr/bin/env python3
"""Remove only checksum-matched retired distribution files; caller holds install lock."""
import argparse
import hashlib
import json
import shutil
from pathlib import Path


def prune(manifest, backup, prefix=Path('/')):
    removed = 0
    for name, digest in json.loads(manifest.read_text()).items():
        relative = Path(name)
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError(f'Invalid retired path: {name}')
        base = 'usr/local/share/fonts/domed' if name.endswith('.bdf') else 'usr/local/share/doc/domed/fonts'
        target = prefix / base / relative
        if target.is_symlink() or any(p.is_symlink() for p in target.parents):
            continue
        if not target.is_file() or hashlib.sha256(target.read_bytes()).hexdigest() != digest:
            continue
        saved = backup / base / relative
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(target, saved)
        target.unlink()
        removed += 1
    return removed


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('backup', type=Path)
    parser.add_argument('--prefix', type=Path, default=Path('/'))
    args = parser.parse_args()
    print(f'Removed {prune(args.manifest, args.backup, args.prefix)} checksum-matched retired font files')
