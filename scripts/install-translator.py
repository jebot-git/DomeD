#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Install a verified release stage, retaining a rollback copy. Never build as root."""
import argparse
from datetime import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument('stage', type=Path)
parser.add_argument('--root', type=Path, default=Path('/'), help='Alternate root for installation tests')
args = parser.parse_args()
stage = args.stage.resolve()
root = args.root.resolve()
if root == Path('/') and os.geteuid() != 0:
    parser.error('Run the prepared install with sudo -A; compilation must remain unprivileged.')
manifest = json.loads((stage / 'translator-manifest.json').read_text())
if not manifest:
    parser.error('Empty install manifest')
for relative, entry in manifest.items():
    p = Path(relative)
    allowed = (relative == 'usr/local/bin/dome-translator' or
               relative.startswith('usr/local/libexec/dome-translator/') or
               relative == 'usr/local/share/applications/org.domed.Translator.desktop' or
               relative == 'usr/local/share/icons/hicolor/scalable/apps/org.domed.Translator.svg' or
               relative == 'usr/local/share/doc/domed/TRANSLATOR.md')
    if p.is_absolute() or '..' in p.parts or not allowed:
        parser.error('Unexpected install path')
    source = stage / relative
    if source.is_symlink() or not source.is_file():
        parser.error('Invalid staged file: ' + relative)
    if hashlib.sha256(source.read_bytes()).hexdigest() != entry['sha256']:
        parser.error('Staged checksum mismatch: ' + relative)
    if entry['mode'] not in (0o644, 0o755):
        parser.error('Invalid file mode')
lock = root / 'run/lock/domed-install.lock'
lock.parent.mkdir(parents=True, exist_ok=True)
backup = root / ('var/backups/domed/translator-' + datetime.now().strftime('%Y%m%d-%H%M%S'))
replaced = []
with lock.open('a') as handle:
    fcntl.flock(handle, fcntl.LOCK_EX)
    try:
        for relative, entry in manifest.items():
            target = root / relative
            # Refuse parent symlinks rather than accidentally writing outside the prefix.
            if any(p.is_symlink() for p in target.parents if p != root):
                raise RuntimeError('Symlinked install directory: ' + str(target))
            original = target.exists() or target.is_symlink()
            if original:
                saved = backup / relative
                saved.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(target, saved, follow_symlinks=False)
            target.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
            fd, temporary = tempfile.mkstemp(prefix='.' + target.name + '-', dir=target.parent)
            try:
                with os.fdopen(fd, 'wb') as output:
                    output.write((stage / relative).read_bytes())
                    output.flush()
                    os.fsync(output.fileno())
                    os.fchmod(output.fileno(), entry['mode'])
                os.replace(temporary, target)
            finally:
                Path(temporary).unlink(missing_ok=True)
            replaced.append((relative, original))
        for relative, entry in manifest.items():
            if hashlib.sha256((root / relative).read_bytes()).hexdigest() != entry['sha256']:
                raise RuntimeError('Installed checksum mismatch: ' + relative)
        destination = root / 'usr/local/share/domed/translator-install.json'
        destination.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
        destination.write_text(json.dumps(manifest, indent=2) + '\n')
        destination.chmod(0o644)
    except BaseException:
        for relative, original in reversed(replaced):
            target = root / relative
            if original:
                os.replace(backup / relative, target)
            else:
                target.unlink(missing_ok=True)
        raise
if root == Path('/') and shutil.which('update-desktop-database'):
    subprocess.run(['update-desktop-database', '/usr/local/share/applications'], check=True)
print(f'Installed and verified {len(manifest)} DomeD Translator files in {root / "usr/local"}.')
if backup.exists():
    print('Previous files backed up at', backup)
print('Active desktop and display-manager sessions were left running.')
