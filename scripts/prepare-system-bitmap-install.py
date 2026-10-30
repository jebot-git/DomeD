#!/usr/bin/env python3
"""Stage a reviewable system bitmap/runtime/browser installation; never elevate."""
import argparse,hashlib,json,os,pathlib,shutil,subprocess,tempfile
P=pathlib.Path
ROOT=P(__file__).resolve().parents[1]
def sha(p):
    with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ['rpm-plan','prefix','toolkit','browser','browser-repo','suite-repo','compositor-dir','output']:p.add_argument('--'+name,type=P,required=True)
    a=p.parse_args()
    if os.geteuid()==0:p.error('Stage as the regular user')
    out=a.output.resolve();out.mkdir(parents=True,exist_ok=True);runtime=out/'runtime'
    marker=out/'.domed-bitmap-stage'
    if runtime.exists():
        if not marker.is_file() or marker.read_text()!='DomeD bitmap installation stage\n':p.error('Use a fresh --output directory; existing runtime is not a task-owned stage')
        previous=P(tempfile.mkdtemp(prefix='runtime-previous-',dir=out));previous.rmdir();runtime.rename(previous)
    marker.write_text('DomeD bitmap installation stage\n')
    shutil.copytree(a.prefix.resolve(),runtime,symlinks=True)
    for file in runtime.rglob('*'):
        if file.is_file() and not file.is_symlink() and file.suffix in ['.pc','.cmake']:
            file.write_text(file.read_text().replace(str(a.prefix.resolve()),'/opt/fltk-vr/bdf-runtime'))
    policy=json.loads((runtime/'share/domed-bdf-runtime/manifest.json').read_text());assert policy['policy']=='native-unicode-bitmap-only' and policy['formats']==['BDF','PCF']
    for name,h in policy['libraries'].items():assert sha(runtime/'lib'/name)==h
    files=[]
    def add(source,destination,mode=0o644,relocate=False):
        source=P(source);destination=P(destination);target=out/'files'/str(destination).lstrip('/');target.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(source,target)
        if relocate:subprocess.run(['patchelf','--set-rpath','/opt/fltk-vr/bdf-runtime/lib:/opt/fltk-vr/toolkit/lib:/usr/local/lib/domed/bitmap',str(target)],check=True)
        target.chmod(mode)
        previous=None; metadata=None
        if destination.exists():
            try:previous=sha(destination)
            except PermissionError:
                # Older installs made some public source notices root-readable
                # only. Record their identity for authenticated verification;
                # executable/library files still require a preflight digest.
                info=destination.stat()
                assert str(destination).startswith('/opt/fltk-vr/toolkit/share/fltk-bdf-source/') and info.st_uid==0 and not info.st_mode&0o111 and not destination.is_symlink()
                metadata={key:getattr(info,key) for key in ['st_dev','st_ino','st_size','st_mtime_ns','st_mode','st_uid']}
        files.append(dict(source=str(target),destination=str(destination),mode=mode,sha256=sha(target),previous_sha256=previous,previous_metadata=metadata))
    add(a.browser,'/usr/local/libexec/dome-browser/dome-browser',0o755,True)
    add(ROOT/'scripts/check-bitmap-runtime.py','/usr/local/libexec/domed/check-bitmap-runtime',0o755)
    add(ROOT/'packaging/99-domed-bitmap.conf','/etc/fonts/conf.d/99-domed-bitmap.conf')
    for source in sorted((a.toolkit/'lib').glob('libfltk*.so.*')):
        if source.is_file() and not source.is_symlink():add(source,'/opt/fltk-vr/toolkit/lib/'+source.name,0o755)
    header=a.toolkit/'include/FL/wayland.H'
    if header.exists():add(header,'/opt/fltk-vr/toolkit/include/FL/wayland.H')
    for source in (a.toolkit/'share/fltk-bdf-source').glob('*'):
        if source.is_file():add(source,'/opt/fltk-vr/toolkit/share/fltk-bdf-source/'+source.name)
    for name in ['dome-compositor','dome-preview','dome-xr','dome-bdf-font','dome-portal-picker']:
        source=a.compositor_dir/name
        if not source.exists():continue
        for folder in ['/usr/local/bin','/opt/fltk-vr/bin','/usr/local/libexec/domed']:
            dest=P(folder)/name
            if dest.is_file() and dest.open('rb').read(4)==b'\x7fELF':add(source,dest,0o755,True)
    wrapper=P('/usr/local/bin/dome-browser').read_text();lines=wrapper.splitlines();matches=[i for i,s in enumerate(lines) if s.startswith('export LD_LIBRARY_PATH=')];assert len(matches)==1
    lines[matches[0]]='export DOME_BDF_RUNTIME_PREFIX="${DOME_BDF_RUNTIME_PREFIX:-$prefix/bdf-runtime}"\nexport LD_LIBRARY_PATH="$DOME_BDF_RUNTIME_PREFIX/lib:$prefix/toolkit/lib:$prefix/lib:$prefix/lib/bitmap${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"'
    temp=out/'dome-browser-wrapper';temp.write_text('\n'.join(lines)+'\n');subprocess.run(['sh','-n',str(temp)],check=True);add(temp,'/usr/local/bin/dome-browser',0o755)
    source=(ROOT/'packaging/dome-session').read_text();start=source.index('# Require the native BDF/PCF stack');end=source.index('export DOME_LOGIN_SESSION',start);block=source[start:end]
    old=P('/usr/local/bin/dome-session').read_text();start=next((old.index(marker) for marker in ['# Require the native BDF/PCF stack','# Only the native BDF fork'] if marker in old),old.index('export DOME_LOGIN_SESSION'))
    end=old.index('export DOME_LOGIN_SESSION',start);temp=out/'dome-session';temp.write_text(old[:start]+block+old[end:]);subprocess.run(['bash','-n',str(temp)],check=True);add(temp,'/usr/local/bin/dome-session',0o755)
    packages=json.loads(a.rpm_plan.read_text());assert len(packages['packages'])==20 and len(packages['abi'])==27 and all(not x['missing_symbols'] for x in packages['abi'])
    # Prevent an unattended stock package update from replacing the selected
    # fork. The explicit staged RPM installer handles subsequent fork updates.
    pin=out/'dnf-bitmap.conf';pin.write_text('[main]\nexcludepkgs='+','.join(sorted({x['name'] for x in packages['packages']}))+'\n');add(pin,'/etc/dnf/libdnf5.conf.d/90-domed-bitmap.conf')
    tree={}
    for file in runtime.rglob('*'):
        if file.is_symlink():tree[str(file.relative_to(runtime))]={'link':str(file.readlink())}
        elif file.is_file():tree[str(file.relative_to(runtime))]={'sha256':sha(file)}
    def commit(repo):return subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True).strip()
    browser=next(x for x in files if x['destination']=='/usr/local/libexec/dome-browser/dome-browser')
    plan=dict(rpm_plan=str(a.rpm_plan.resolve()),rpm_plan_sha256=sha(a.rpm_plan),runtime_source=str(runtime),runtime_tree=tree,files=files,browser_sha256=browser['sha256'],commits={'DomeD':commit(ROOT),'dome-browser':commit(a.browser_repo),'fltk-vr':commit(a.suite_repo),'harfbuzz':json.loads((ROOT/'native/bitmap-runtime/sources.json').read_text())['harfbuzz_revision']})
    target=out/'install-plan.json';target.write_text(json.dumps(plan,indent=2)+'\n');print('Staged',len(files),'system files and 20 RPMs. Plan SHA256:',sha(target))
if __name__=='__main__':main()
