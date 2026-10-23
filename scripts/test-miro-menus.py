#!/usr/bin/env python3
"""Run the maintained native FLTK Miro integration test from its fork."""
import os
from pathlib import Path
import sys
root = Path(__file__).resolve().parents[1]
source = Path(os.environ.get('DOME_MIRO_SOURCE', root.parent / 'miro'))
test = source / 'scripts/test-dome.py'
if not test.is_file():
    test = root / 'build/miro-fltk-source/scripts/test-dome.py'
if not test.is_file():
    raise SystemExit('Native Miro sources missing; run scripts/install-miro.sh or set DOME_MIRO_SOURCE')
import shutil
os.environ.setdefault('MIRO_BINARY', os.environ.get('DOME_MIRO', shutil.which('miro-pdf') or str(Path.home() / '.local/bin/miro-pdf')))
os.environ.setdefault('DOME_COMPOSITOR', str(root / 'target/release/dome-compositor'))
os.execv(sys.executable, [sys.executable, str(test), *sys.argv[1:]])
