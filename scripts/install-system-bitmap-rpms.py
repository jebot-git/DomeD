#!/usr/bin/env python3
"""Install a checksum-verified bitmap RPM plan and staged application files."""
import argparse,ctypes,datetime,fcntl,hashlib,json,os,pathlib,shutil,subprocess,tempfile
P=pathlib.Path

def sha(p):
    with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def verify_tree(root,expected):
    actual={}
    for p in root.rglob('*'):
        if p.is_symlink():actual[str(p.relative_to(root))]={'link':str(p.readlink())}
        elif p.is_file():actual[str(p.relative_to(root))]={'sha256':sha(p)}
    assert actual==expected,'Staged runtime differs from its reviewed manifest'
def public_tree(root):
    root.chmod(0o755)
    for p in root.rglob('*'):
        if not p.is_symlink():p.chmod(0o755 if p.is_dir() or p.stat().st_mode&0o111 else 0o644)
def replace_file(source,destination,mode):
    destination.parent.mkdir(parents=True,exist_ok=True,mode=0o755)
    temp=destination.with_name('.'+destination.name+'.domed-new')
    shutil.copy2(source,temp);temp.chmod(mode);os.chown(temp,0,0);os.replace(temp,destination)
def exchange_directories(staged,destination,backup):
    if destination.exists():
        libc=ctypes.CDLL(None,use_errno=True);fn=libc.renameat2
        fn.argtypes=[ctypes.c_int,ctypes.c_char_p,ctypes.c_int,ctypes.c_char_p,ctypes.c_uint]
        if fn(-100,os.fsencode(staged),-100,os.fsencode(destination),2):raise OSError(ctypes.get_errno(),'Atomic runtime exchange failed')
        shutil.move(str(staged),backup)
    else:os.replace(staged,destination)
def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('plan',type=P);parser.add_argument('sha256');args=parser.parse_args()
    if os.geteuid()!=0:parser.error('Administrator authentication is required for system installation')
    os.umask(0o022)
    lock=open('/run/lock/domed-bitmap-install.lock','w');fcntl.flock(lock,fcntl.LOCK_EX)
    assert sha(args.plan)==args.sha256,'Installation plan changed'
    plan=json.loads(args.plan.read_text());base=args.plan.resolve().parent
    packages=json.loads(P(plan['rpm_plan']).read_text());assert sha(P(plan['rpm_plan']))==plan['rpm_plan_sha256']
    for item in packages['packages']:
        assert sha(P(item['archive']))==item['sha256'] and sha(P(item['original']))==item['original_sha256']
        installed=subprocess.check_output(['rpm','-q','--qf','%{VERSION}-%{RELEASE}',item['name']+'.'+item['arch']],text=True).strip()
        assert installed==item['version']+'-'+item['release'],item['name']
    for item in plan['files']:
        assert sha(P(item['source']))==item['sha256'],item['source']
        destination=P(item['destination'])
        if item.get('previous_sha256'):assert sha(destination)==item['previous_sha256'],str(destination)+' changed since staging'
        elif item.get('previous_metadata'):
            info=destination.stat()
            assert {key:getattr(info,key) for key in item['previous_metadata']}==item['previous_metadata'],str(destination)+' changed since staging'
            assert str(destination).startswith('/opt/fltk-vr/toolkit/share/fltk-bdf-source/') and info.st_uid==0 and not info.st_mode&0o111 and not destination.is_symlink()
        else:assert not destination.exists(),str(destination)+' appeared since staging'
    verify_tree(P(plan['runtime_source']),plan['runtime_tree'])
    # Validate the full package transaction again under authentication.
    archives=[item['archive'] for item in packages['packages']]
    subprocess.run(['rpm','-Uvh','--test',*archives],check=True)
    stamp=datetime.datetime.now().strftime('%Y%m%d-%H%M%S')
    backup=P('/var/backups/de-xr')/('bitmap-runtime-'+stamp);backup.mkdir(parents=True,mode=0o700)
    originals=backup/'rpms';originals.mkdir()
    for item in packages['packages']:shutil.copy2(item['original'],originals/P(item['original']).name)
    shutil.copy2(args.plan,backup/'install-plan.json')
    for item in plan['files']:
        destination=P(item['destination'])
        if destination.exists() or destination.is_symlink():
            target=backup/'files'/str(destination).lstrip('/');target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(destination,target,follow_symlinks=False)
    destination=P('/opt/fltk-vr/bdf-runtime');staged=destination.with_name('.bdf-runtime-'+stamp)
    shutil.copytree(plan['runtime_source'],staged,symlinks=True);public_tree(staged);verify_tree(staged,plan['runtime_tree'])
    result=dict(status='backed-up',backup=str(backup),sessions_restarted=False,applications_restarted=False)
    (base/'install-result.json').write_text(json.dumps(result,indent=2)+'\n')
    subprocess.run(['rpm','-Uvh',*archives],check=True)
    exchange_directories(staged,destination,backup/'previous-private-runtime')
    for item in plan['files']:replace_file(P(item['source']),P(item['destination']),item['mode'])
    # Legacy launchers still prepend these locations. Redirect all duplicate
    # font DSOs atomically, so a newly launched application cannot pick an old
    # private Pango or HarfBuzz. Existing mapped pages remain valid.
    redirects=[]
    for directory in [P('/opt/fltk-vr/toolkit/lib'),P('/usr/local/lib/domed/bitmap'),P('/opt/fltk-vr/lib/bitmap')]:
        if not directory.exists():continue
        for prefix in ['libpango','libharfbuzz','libfreetype','libcairo']:
            for old in directory.glob(prefix+'*.so*'):
                # Match the SONAME family against the newly installed runtime.
                target=next((p for p in (destination/'lib').glob(prefix+'*.so*') if p.name.split('.so')[0]==old.name.split('.so')[0] and p.name.endswith('.so')),None)
                if target is None:continue
                saved=backup/'legacy'/str(old).lstrip('/');saved.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(old,saved,follow_symlinks=False)
                temp=old.with_name('.'+old.name+'.domed-link');temp.symlink_to(target);os.replace(temp,old);redirects.append(str(old))
    subprocess.run(['ldconfig'],check=True)
    for item in packages['new_libraries']:assert sha(P(item['path']))==item['sha256'],item['path']
    for item in plan['files']:assert sha(P(item['destination']))==item['sha256']
    clean={k:v for k,v in os.environ.items() if k not in ['LD_LIBRARY_PATH','LD_PRELOAD']}
    checker='/usr/local/libexec/domed/check-bitmap-runtime'
    for manifest,env in [(P('/usr/share/domed-bdf-runtime/manifest.json'),clean),(destination/'share/domed-bdf-runtime/manifest.json',{**clean,'LD_LIBRARY_PATH':str(destination/'lib')})]:
        subprocess.run([checker,'--manifest',str(manifest)],env=env,check=True)
    for item in packages['packages']:
        installed=subprocess.check_output(['rpm','-q','--qf','%{VERSION}-%{RELEASE}',item['name']+'.'+item['arch']],text=True).strip()
        assert installed==item['version']+'-'+item['new_release']
    result.update(status='installed',commits=plan['commits'],packages=len(packages['packages']),legacy_redirects=redirects,system_and_private_policy_checks='passed',browser_sha256=plan['browser_sha256'])
    receipt=P('/usr/local/share/dome-browser/bitmap-runtime-install.json');receipt.parent.mkdir(parents=True,exist_ok=True);receipt.write_text(json.dumps(result,indent=2)+'\n');receipt.chmod(0o644)
    (base/'install-result.json').write_text(json.dumps(result,indent=2)+'\n');(base/'install-result.json').chmod(0o644)
    print('Installed BDF/PCF system libraries and browser. Rollback:',backup)
if __name__=='__main__':main()
