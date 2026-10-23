#!/usr/bin/env python3
"""Check DomeD-owned widget frontends and their dynamic toolkit dependencies.
Third-party application/engine daemons are outside this frontend inventory.
"""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--prefix', type=Path, default=Path('/usr/local'))
parser.add_argument('--compositor', type=Path)
parser.add_argument('--output', type=Path, default=root/'artifacts/fltk-frontends.json')
parser.add_argument('--source-only', action='store_true')
args = parser.parse_args()
checks = []
def check(ok, description, **details):
    checks.append(dict(passed=bool(ok), check=description, **details))
    print(('PASS ' if ok else 'FAIL ')+description, flush=True)

cargo = (root/'Cargo.toml').read_text()
check('qt6 =' not in cargo, 'Cargo does not offer a Qt frontend')
check('Qt/GTK frontend fallbacks are unsupported' in (root/'build.rs').read_text(), 'Native builds reject a missing FLTK frontend')
check('DOME_TRAY_FRONTEND' not in (root/'src/bin/dome-compositor/session_services.rs').read_text() and
      'DOME_TRAY_FRONTEND' not in (root/'src/bin/dome-compositor/tray.rs').read_text(),
      'Legacy tray preferences cannot bypass native FLTK frontends')
check('default=fltk' in (root/'packaging/dome-portals.conf').read_text(), 'DomeD portal dialogs select FLTK')
check('polkit-kde-authentication-agent' not in (root/'src/bin/dome-compositor/session_services.rs').read_text(), 'DomeD does not launch a Qt authentication fallback')
check(not any((root/name).exists() for name in ('native/qt_chrome.cpp','native/default_apps.cpp','native/portal_picker.cpp')), 'Alternate Qt widget renderer sources are removed')

# Audio, clipboard, notification history, clock and keyboard selector all use
# the compositor's same compiled FLTK renderer; they are not external widgets.
roles = {
 'shell/audio/clipboard/notifications/keyboard': args.compositor or args.prefix/'bin/dome-compositor',
 'tray applications': args.prefix/'bin/dome-tray',
 'input configuration': args.prefix/'bin/fcitx5-configuration-fltk',
 'screen sharing picker': args.prefix/'bin/dome-portal-picker',
 'password prompt': args.prefix/'libexec/domed/pinentry-fltk',
 'authentication agent': args.prefix/'libexec/domed/dome-polkit-agent',
 'desktop portal dialogs': args.prefix/'libexec/domed/xdg-desktop-portal-fltk',
}
translator=args.prefix/'bin/dome-translator'
if translator.exists(): roles['translator']=translator
bad = re.compile(r'lib(?:Qt[456]|gtk[-.]|gdk[-.]|pango|tk[0-9.]|wx_)',re.I)
if not args.source_only:
    configs = [Path('/etc/xdg/xdg-desktop-portal/dome-portals.conf'),
               Path('/usr/share/xdg-desktop-portal/dome-portals.conf')]
    config = next((p for p in configs if p.is_file()), None)
    check(config is not None and 'default=fltk' in config.read_text(),
          'Installed DomeD portal preference selects FLTK', path=str(config))
    for role,path in roles.items():
        if not path.is_file():
            check(False,role+' is installed',path=str(path));continue
        result=subprocess.run(['ldd',str(path)],capture_output=True,text=True)
        libraries=result.stdout+result.stderr
        forbidden=[line.strip() for line in libraries.splitlines() if bad.search(line) or 'not found' in line]
        check(result.returncode==0 and 'libfltk' in libraries and not forbidden,
              role+' uses FLTK without another widget toolkit',path=str(path),unexpected=forbidden)
args.output.parent.mkdir(parents=True,exist_ok=True)
args.output.write_text(json.dumps({'checks':checks,'failed':sum(not c['passed'] for c in checks)},indent=2)+'\n')
sys.exit(any(not c['passed'] for c in checks))
