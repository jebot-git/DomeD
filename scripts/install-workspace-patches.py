#!/usr/bin/env python3
"""Install only prepared release patches; preserve sessions and retain backups."""
import argparse,datetime,fcntl,hashlib,json,os,shutil,tempfile
from pathlib import Path
ALLOWED={
'opt/fltk-vr/bin/dome-compositor','usr/local/bin/dome-tray','usr/local/bin/dome-session',
'usr/local/bin/fltube','usr/local/share/doc/fltube/README.md',
'usr/local/share/applications/org.domed.Connect.desktop',
'usr/local/share/applications/org.domed.InputMethods.desktop',
'usr/local/share/applications/org.domed.Wallets.desktop',
'usr/local/share/doc/domed/TRAY_FRONTENDS.md','usr/local/share/doc/domed/FLTK_GPU_AUDIT.md',
'opt/fltk-wpe-xr/fltk-wpe/build/fltk-wpe','opt/fltk-wpe-xr/share/doc/GPU_PRESENTATION.md',
'opt/fltk-wpe-xr/install-manifest.json'}
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('stage',type=Path);parser.add_argument('--verify-only',action='store_true');args=parser.parse_args()
    stage=args.stage.resolve();manifest=json.loads((stage/'patch-manifest.json').read_text())
    if len(manifest)!=len(ALLOWED) or {item['path'] for item in manifest}!=ALLOWED:raise SystemExit('Unexpected patch file list')
    for item in manifest:
        source=stage/item['path'];mode=item['mode']
        if source.is_symlink() or not source.is_file() or digest(source)!=item['sha256'] or mode not in (0o644,0o755):raise SystemExit('Invalid staged file: '+item['path'])
    if args.verify_only:print('Verified',len(manifest),'staged release patch files');return
    if os.geteuid()!=0:raise SystemExit('Use sudo -A for this prepared system installation')
    os.umask(0o022)
    lock=open('/run/lock/domed-install.lock','w');fcntl.flock(lock,fcntl.LOCK_EX)
    backup=Path('/var/backups/de-xr')/datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    # Back up everything before replacing the first file.
    for item in manifest:
        target=Path('/')/item['path']
        if target.exists() or target.is_symlink():
            saved=backup/item['path'];saved.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(target,saved,follow_symlinks=False)
    for item in manifest:
        source=stage/item['path'];target=Path('/')/item['path'];target.parent.mkdir(parents=True,exist_ok=True,mode=0o755)
        # Do not expose partially copied executables or truncate a running binary.
        fd,temporary=tempfile.mkstemp(prefix='.'+target.name+'.patch-',dir=target.parent)
        try:
            with os.fdopen(fd,'wb') as output:
                output.write(source.read_bytes());output.flush();os.fsync(output.fileno());os.fchmod(output.fileno(),item['mode'])
            os.chown(temporary,0,0);os.replace(temporary,target)
        finally:
            if Path(temporary).exists():Path(temporary).unlink()
        if digest(target)!=item['sha256']:raise SystemExit('Installed checksum mismatch: '+str(target))
    receipt=Path('/usr/local/share/domed/workspace-patch-install.json');receipt.parent.mkdir(parents=True,exist_ok=True)
    receipt.write_text(json.dumps(manifest,indent=2)+'\n');receipt.chmod(0o644)
    import subprocess
    if shutil.which('restorecon'):subprocess.run(['restorecon',*[str(Path('/')/item['path']) for item in manifest]],check=True)
    if shutil.which('update-desktop-database'):subprocess.run(['update-desktop-database','/usr/local/share/applications'],check=True)
    print('Installed and verified',len(manifest),'release patch files. Backup:',backup)
    print('Existing desktop, browser and display-manager sessions were left running.')
if __name__=='__main__':main()
