#!/usr/bin/env python3
"""Build pinned stable FLTK plus the experiment's reviewed local patches."""
import argparse
import hashlib
import os
import shutil
import tempfile
from pathlib import Path
import subprocess
import tarfile
import urllib.request

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--deps',type=Path)
p.add_argument('--prefix',type=Path)
a=p.parse_args()
repo=Path(__file__).resolve().parents[1]
suite=repo.parent
deps=(a.deps or suite/'_deps').resolve();prefix=(a.prefix or suite/'_toolkit').resolve()
deps.mkdir(parents=True,exist_ok=True)
archive=deps/'fltk-1.4.5-source.tar.gz'
if not archive.exists():
    urllib.request.urlretrieve('https://github.com/fltk/fltk/releases/download/release-1.4.5/'+archive.name,archive)
expected='eede1fb2b8e9c2e581e77082e15252145855c79aad30070ee3b24aabe2f926f1'
if hashlib.sha256(archive.read_bytes()).hexdigest()!=expected:
    raise SystemExit('FLTK source archive checksum mismatch')
source=deps/'fltk-1.4.5'
patch_names=['fltk-1.4.5-bitmap-fonts.patch','fltk-1.4.5-popup-done.patch','fltk-1.4.5-depth-buffer.patch','fltk-1.4.5-borderless-app-id.patch','fltk-1.4.5-bitmap-styles.patch','fltk-1.4.5-window-close.patch','fltk-1.4.5-first-configure.patch','fltk-1.4.5-empty-gl-text.patch','fltk-1.4.5-bitmap-refresh.patch','fltk-1.4.5-bdf-only.patch','fltk-1.4.5-bdf-ink.patch','fltk-1.4.5-freetype-only.patch','fltk-1.4.5-freetype-decor.patch','fltk-1.4.5-bdf-fallback.patch','fltk-1.4.5-compositor-fullscreen.patch']
patches=[repo/'native/toolkit-hooks'/name for name in patch_names]
digest=hashlib.sha256(b''.join(p.read_bytes() for p in patches)).hexdigest()
stamp=source/'.dome-patches.sha256'
if not stamp.is_file() or stamp.read_text().strip()!=digest:
    # Preserve the previous task-owned source tree, then patch a verified clean
    # archive. Overlapping patches cannot be detected reliably with reverse dry-runs.
    if source.exists():
        backup=Path(tempfile.mkdtemp(prefix='fltk-1.4.5-previous-',dir=deps))
        backup.rmdir();source.rename(backup)
        print('Preserved previous toolkit sources:',backup,flush=True)
    with tarfile.open(archive) as f:f.extractall(deps,filter='data')
    for patch in patches:
        subprocess.run(['patch','--batch','--force','--forward','-p1','-i',str(patch)],cwd=source,check=True)
    stamp.write_text(digest+'\n')
env=dict(os.environ)
sysroot=deps/'sysroot/usr'
if sysroot.is_dir():
    env['PKG_CONFIG_PATH']=str(sysroot/'lib/x86_64-linux-gnu/pkgconfig')+':'+env.get('PKG_CONFIG_PATH','')
args=['cmake','-S',str(source),'-B',str(deps/'build'),'-G','Ninja','-DCMAKE_BUILD_TYPE=Release',f'-DCMAKE_INSTALL_PREFIX={prefix}',f'-DCMAKE_PREFIX_PATH={sysroot}','-DFLTK_BACKEND_WAYLAND=ON','-DFLTK_BACKEND_X11=ON','-DFLTK_BUILD_SHARED_LIBS=ON','-DFLTK_BUILD_TEST=OFF','-DFLTK_BUILD_EXAMPLES=OFF','-DFLTK_BUILD_FLUID=OFF','-DFLTK_BUILD_FLTK_OPTIONS=OFF','-DFLTK_BUILD_FORMS=OFF','-DFLTK_BUILD_GL=ON','-DFLTK_USE_DBUS=OFF','-DFLTK_USE_PANGO=OFF','-DFLTK_USE_SYSTEM_LIBDECOR=OFF','-DFLTK_USE_LIBDECOR_GTK=OFF','-DFLTK_OPTION_CAIRO_WINDOW=ON']
subprocess.run(args,env=env,check=True)
subprocess.run(['cmake','--build',str(deps/'build'),'--parallel','4'],check=True)
subprocess.run(['cmake','--install',str(deps/'build')],check=True)
notices=prefix/'share/fltk-bdf-source'
notices.mkdir(parents=True,exist_ok=True)
for item in [archive,source/'COPYING',*patches]:shutil.copy2(item,notices/item.name)
print('Isolated FLTK 1.4.5:',prefix)
