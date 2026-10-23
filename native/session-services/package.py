#!/usr/bin/env python3
"""Package the native services and the installed KDE Connect backend separately.

The backend package preserves the distributor's binaries, plugins and copyright
notices. Re-run this script against a newer installed KDE Connect version when
refreshing it; the package records that upstream version.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--build', type=Path, required=True)
args = parser.parse_args()
OUT = args.output.resolve()
OUT.mkdir(parents=True, exist_ok=True)

def write(root, path, content, mode=0o644):
    dest = root/path.lstrip('/')
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.is_symlink():
        dest.unlink()
    dest.write_text(content)
    dest.chmod(mode)

def copy(root, path, target=None):
    dest = root/(target or str(path)).lstrip('/')
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() or dest.is_symlink():
        dest.unlink()
    # Documentation symlinks can point into the GUI package being removed.
    # Preserve their contents in the self-contained backend package.
    shutil.copy2(path, dest, follow_symlinks=True)
    if not dest.is_symlink():
        dest.chmod(0o755 if os.access(path, os.X_OK) else 0o644)

def package(root, name, version):
    root.chmod(0o755)
    for path in root.rglob('*'):
        if path.is_dir() and not path.is_symlink():
            path.chmod(0o755)
    files = sorted(p for p in root.rglob('*') if p.is_file() and 'DEBIAN' not in p.parts)
    checksums = ''.join(hashlib.md5(p.read_bytes()).hexdigest()+'  '+str(p.relative_to(root))+'\n'
                        for p in files if not p.is_symlink())
    write(root, 'DEBIAN/md5sums', checksums)
    control = root/'DEBIAN/control'
    control.write_text(control.read_text()+f'Installed-Size: {sum(p.lstat().st_size for p in files)//1024}\n')
    # Root ownership is encoded in the archive; no build-time privilege is needed.
    output = OUT/f'{name}_{version}_amd64.deb'
    subprocess.run(['dpkg-deb','--root-owner-group','--build',str(root),str(output)],check=True)
    return output

backend = OUT/'backend-stage'
backend.mkdir(exist_ok=True)
version = subprocess.check_output(['dpkg-query','-W','-f=${Version}','kdeconnect'],text=True).strip()
paths = subprocess.check_output(['dpkg-query','-L','kdeconnect'],text=True).splitlines()
keep_bins = {'/usr/bin/kdeconnectd','/usr/bin/kdeconnect-cli','/usr/bin/kdeconnect-handler'}
keep_exact = keep_bins | {
    '/etc/ufw/applications.d/kdeconnect',
    '/etc/xdg/autostart/org.kde.kdeconnect.daemon.desktop',
    '/usr/share/dbus-1/services/org.kde.kdeconnect.service',
    '/usr/share/applications/org.kde.kdeconnect.daemon.desktop',
    '/usr/share/applications/org.kde.kdeconnect.handler.desktop',
}
keep_prefixes = ('/usr/lib/x86_64-linux-gnu/qt6/plugins/kdeconnect/',
                 '/usr/lib/x86_64-linux-gnu/qt6/plugins/kf6/kio/',
                 '/usr/share/icons/', '/usr/share/knotifications6/',
                 '/usr/share/qlogging-categories6/', '/usr/share/doc/kdeconnect/')
keep_translations = {'kdeconnect-cli.mo','kdeconnect-core.mo','kdeconnect-interfaces.mo',
                     'kdeconnect-kded.mo','kdeconnect-kio.mo','kdeconnect-plugins.mo','kdeconnect-urlhandler.mo'}
kept = []
for path in paths:
    p = Path(path)
    if not p.is_file() and not p.is_symlink():
        continue
    if path in keep_exact or path.startswith(keep_prefixes) or ('/locale/' in path and p.name in keep_translations):
        copy(backend,path);kept.append(path)
shlib = OUT/'shlibs'
(shlib/'debian').mkdir(parents=True,exist_ok=True)
write(shlib,'debian/control','Source: domed-kdeconnect-backend\nMaintainer: DomeD Local Integration <root@localhost>\n\nPackage: domed-kdeconnect-backend\nArchitecture: amd64\nDescription: KDE Connect backend for DomeD\n')
elfs=[p for p in kept if p in keep_bins or p.endswith('.so')]
depends=subprocess.check_output(['dpkg-shlibdeps','-O',*['-e'+p for p in elfs]],cwd=shlib,text=True).strip().split('=',1)[1]
depends += ', libqca-qt6-plugins, sshfs (>= 3)'
write(backend,'DEBIAN/control',f'''Package: domed-kdeconnect-backend
Version: {version}+domed1
Architecture: amd64
Section: net
Priority: optional
Maintainer: DomeD Local Integration <root@localhost>
Depends: {depends}
Provides: kdeconnect (= {version})
Conflicts: kdeconnect
Replaces: kdeconnect
Description: KDE Connect backend for the DomeD FLTK interface
 Preserves the distributor's daemon, CLI, transfer helpers and protocol plugins.
 Device management is provided by DomeD's FLTK tray instead of Qt applications.
 Upstream backend version: {version}.
''')
write(backend,'DEBIAN/conffiles','/etc/ufw/applications.d/kdeconnect\n/etc/xdg/autostart/org.kde.kdeconnect.daemon.desktop\n')
backend_deb=package(backend,'domed-kdeconnect-backend',version+'+domed1')

services=OUT/'services-stage';services.mkdir(exist_ok=True)
for binary in ['dome-polkit-agent','xdg-desktop-portal-fltk']:
    copy(services,args.build/binary,'/usr/local/libexec/domed/'+binary)
write(services,'/usr/bin/pinentry-fltk','')
(services/'usr/bin/pinentry-fltk').unlink()
(services/'usr/bin/pinentry-fltk').symlink_to('/usr/local/libexec/domed/pinentry-fltk')
for path in (ROOT/'interfaces').glob('*'):
    copy(services,path,'/usr/local/share/domed/portal-interfaces/'+path.name)
interfaces=['FileChooser','Access','AppChooser','Print','Notification','Email','Account','DynamicLauncher','Lockdown','Inhibit']
write(services,'/usr/share/xdg-desktop-portal/portals/fltk.portal','[portal]\nDBusName=org.freedesktop.impl.portal.desktop.fltk\nInterfaces='+''.join('org.freedesktop.impl.portal.'+i+';' for i in interfaces)+'\nUseIn=Dome;\n')
write(services,'/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.fltk.service',
      '[D-BUS Service]\nName=org.freedesktop.impl.portal.desktop.fltk\nExec=/usr/local/libexec/domed/xdg-desktop-portal-fltk\nSystemdService=xdg-desktop-portal-fltk.service\n')
write(services,'/usr/lib/systemd/user/xdg-desktop-portal-fltk.service',
      '[Unit]\nDescription=DomeD FLTK desktop portal\nPartOf=dome-session.target\nAfter=graphical-session.target\n\n[Service]\nType=dbus\nBusName=org.freedesktop.impl.portal.desktop.fltk\nEnvironment=XDG_SESSION_ID=\nEnvironment=DOME_PORTAL_INTERFACES=/usr/local/share/domed/portal-interfaces\nExecStart=/usr/local/libexec/domed/xdg-desktop-portal-fltk\nRestart=on-failure\n')
write(services,'/usr/lib/systemd/user/dome-polkit-agent.service',
      '[Unit]\nDescription=DomeD FLTK authentication agent\nPartOf=dome-session.target\nAfter=graphical-session.target\n\n[Service]\nType=simple\nEnvironment=XDG_SESSION_ID=\nExecStart=/usr/local/libexec/domed/dome-polkit-agent\nRestart=on-failure\n\n[Install]\nWantedBy=dome-session.target\n')
write(services,'/etc/systemd/user/dome-session.target.wants/dome-polkit-agent.service','')
(services/'etc/systemd/user/dome-session.target.wants/dome-polkit-agent.service').unlink()
(services/'etc/systemd/user/dome-session.target.wants/dome-polkit-agent.service').symlink_to('/usr/lib/systemd/user/dome-polkit-agent.service')
write(services,'/etc/xdg/xdg-desktop-portal/dome-portals.conf',
      '[preferred]\ndefault=fltk\norg.freedesktop.impl.portal.ScreenCast=dome-screencast\norg.freedesktop.impl.portal.Settings=dome-settings\norg.freedesktop.impl.portal.Secret=kwallet\norg.freedesktop.impl.portal.RemoteDesktop=none\norg.freedesktop.impl.portal.InputCapture=none\norg.freedesktop.impl.portal.Screenshot=none\norg.freedesktop.impl.portal.GlobalShortcuts=none\n')
write(services,'/etc/environment.d/60-domed-terminal.conf',
      'DOME_TERMINAL_BACKEND=flterm\nTERMINAL=/usr/local/bin/flterm\nTERMINAL_EMULATOR=/usr/local/bin/flterm\nSSH_ASKPASS=/usr/local/bin/dome-askpass\nPINENTRY_BINARY=/usr/local/libexec/domed/pinentry-fltk\n')
write(services,'/etc/profile.d/domed-terminal.sh',
      'export DOME_TERMINAL_BACKEND=flterm\nexport TERMINAL=/usr/local/bin/flterm\nexport TERMINAL_EMULATOR=/usr/local/bin/flterm\nexport SSH_ASKPASS=/usr/local/bin/dome-askpass\nexport PINENTRY_BINARY=/usr/local/libexec/domed/pinentry-fltk\n')
write(services,'/etc/xdg/xdg-terminals.list','org.domed.FLTerm.desktop\n')
write(services,'/usr/local/bin/flterm','''#!/usr/bin/python3
"""Desktop terminal entry point with the standard -e command convention."""
import os, sys
args=sys.argv[1:]
i=0
while i<len(args):
    arg=args[i]
    if arg=='--':break
    if arg in ['-e','--execute']:
        args[i]='--';break
    if arg=='--working-directory':
        os.chdir(args[i+1]);del args[i:i+2];continue
    if arg.startswith('--working-directory='):
        os.chdir(arg.split('=',1)[1]);del args[i];continue
    i+=1
os.execv('/usr/local/bin/flterm-xr',['flterm-xr','--wayland',*args])
''',0o755)
write(services,'/usr/local/bin/fcitx5-configtool','#!/bin/sh\nexec /usr/local/bin/fcitx5-configuration-fltk "$@"\n',0o755)
# Compatibility launchers open the existing FLTK wallet page. The storage
# daemons and their credential/authorization dialogs are separate components.
write(services,'/usr/local/bin/kwalletmanager5','#!/bin/sh\nexec /usr/local/bin/dome-tray --wallets\n',0o755)
write(services,'/usr/local/share/applications/kwalletmanager5-kwalletd.desktop',
      '[Desktop Entry]\nType=Application\nName=Wallet Manager (FLTK)\nExec=/usr/local/bin/dome-tray --wallets\nTryExec=/usr/local/bin/dome-tray\nNoDisplay=true\nTerminal=false\n')
write(services,'/usr/local/bin/kdeconnect-app','#!/bin/sh\nexec /usr/local/bin/dome-tray --devices\n',0o755)
write(services,'/usr/local/bin/kdeconnect-indicator','#!/bin/sh\nexec /usr/local/bin/dome-tray --devices\n',0o755)
write(services,'/usr/local/bin/sensible-terminal-dome','#!/bin/sh\nexec /usr/local/bin/flterm "$@"\n',0o755)
write(services,'/usr/share/applications/org.domed.FLTerm.Default.desktop',
      '[Desktop Entry]\nType=Application\nName=FLTerm\nExec=/usr/local/bin/flterm\nTryExec=/usr/local/bin/flterm\nIcon=utilities-terminal\nTerminal=false\nCategories=System;TerminalEmulator;\nNoDisplay=true\nX-TerminalArgExec=--\nX-TerminalArgDir=--working-directory\n')
write(services,'/etc/xdg/xdg-terminals.list','org.domed.FLTerm.Default.desktop\norg.domed.FLTerm.desktop\n')
write(services,'DEBIAN/control','''Package: domed-session-services
Version: 0.1.0
Architecture: amd64
Section: x11
Priority: optional
Maintainer: DomeD Local Integration <root@localhost>
Depends: libc6, libstdc++6, libglib2.0-0t64, libpolkit-agent-1-0, libpolkit-gobject-1-0, polkitd, cups-client, xdg-utils, python3, xdg-desktop-portal
Provides: pinentry, polkit-1-auth-agent, xdg-desktop-portal-backend
Conflicts: pinentry-qt, polkit-kde-agent-1, xdg-desktop-portal-gtk
Replaces: pinentry-qt, polkit-kde-agent-1, xdg-desktop-portal-gtk
Description: Native FLTK authentication, portal and terminal integration for DomeD
 Uses the installed, patched DomeD FLTK toolkit and BDF font resources.
''')
write(services,'DEBIAN/postinst','''#!/bin/sh
set -e
update-alternatives --install /usr/bin/pinentry pinentry /usr/bin/pinentry-fltk 100
update-alternatives --set pinentry /usr/bin/pinentry-fltk
update-alternatives --install /usr/bin/pinentry-x11 pinentry-x11 /usr/bin/pinentry-fltk 100
update-alternatives --set pinentry-x11 /usr/bin/pinentry-fltk
update-alternatives --install /usr/bin/x-terminal-emulator x-terminal-emulator /usr/local/bin/flterm 100
update-alternatives --set x-terminal-emulator /usr/local/bin/flterm
''',0o755)
write(services,'DEBIAN/prerm','''#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = deconfigure ]; then
 update-alternatives --remove pinentry /usr/bin/pinentry-fltk
 update-alternatives --remove pinentry-x11 /usr/bin/pinentry-fltk
 update-alternatives --remove x-terminal-emulator /usr/local/bin/flterm
fi
''',0o755)
service_deb=package(services,'domed-session-services','0.1.0')
(OUT/'packages.json').write_text(json.dumps({'backend':str(backend_deb),'services':str(service_deb),'backend_upstream_version':version,'retained_backend_files':kept},indent=2))
print(backend_deb,service_deb,sep='\n')
