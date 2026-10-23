#!/usr/bin/env python3
"""Build native FLTK session services against the installed DomeD toolkit."""
import argparse
import os
from pathlib import Path
import shlex
import subprocess

ROOT = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--build', type=Path, required=True)
parser.add_argument('--polkit-headers', type=Path, required=True)
parser.add_argument('--only', choices=['polkit', 'portal'])
args = parser.parse_args()
args.build.mkdir(parents=True, exist_ok=True)
prefix = Path(os.environ.get('DOME_FLTK_PREFIX', '/opt/fltk-vr/toolkit'))
def flags(*cmd):
    return shlex.split(subprocess.check_output(cmd, text=True))
common = ['g++', '-std=c++20', '-O2', '-Wall', '-Wextra', '-Wno-missing-field-initializers',
          '-I' + str(ROOT.parent), *flags(str(prefix/'bin/fltk-config'), '--cxxflags'),
          *flags('pkg-config', '--cflags', 'gio-unix-2.0', 'fontconfig', 'zlib')]
libraries = [*flags(str(prefix/'bin/fltk-config'), '--use-gl', '--use-images', '--ldflags'),
             *flags('pkg-config', '--libs', 'gio-unix-2.0', 'fontconfig', 'zlib'),
             '-Wl,-rpath,' + str(prefix/'lib')]
for source, name in [('polkit-agent.cpp', 'dome-polkit-agent'), ('portal.cpp', 'xdg-desktop-portal-fltk')]:
    if args.only and source != ('polkit-agent.cpp' if args.only == 'polkit' else 'portal.cpp'):
        continue
    if not (ROOT/source).exists():
        continue
    command = common + ['-I' + str(args.polkit_headers), str(ROOT/source), '-o', str(args.build/name)]
    if source == 'polkit-agent.cpp':
        command += ['/usr/lib/x86_64-linux-gnu/libpolkit-agent-1.so.0',
                    '/usr/lib/x86_64-linux-gnu/libpolkit-gobject-1.so.0']
    else:
        command += ['-DDOME_PORTAL_INTERFACES="' + str(ROOT/'interfaces') + '"']
    subprocess.run(command + libraries, check=True)
