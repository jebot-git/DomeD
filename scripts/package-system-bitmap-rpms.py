#!/usr/bin/env python3
"""Stage version-aligned RPM replacements, retaining the installed package payloads."""
import argparse, hashlib, json, os, pathlib, re, shlex, shutil, subprocess
P = pathlib.Path

def run(args, **kw): return subprocess.check_output([str(x) for x in args], text=True, **kw)
def sha(p):
    with p.open('rb') as f: return hashlib.file_digest(f, 'sha256').hexdigest()
def exports(p): return {l.split()[-1].split('@')[0] for l in run(['nm','-D','--defined-only',p]).splitlines() if len(l.split()) >= 3}
def query(p, fmt): return run(['rpm','-qp','--qf',fmt,p]).strip()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix',type=P,required=True)
    parser.add_argument('--prefix32',type=P,required=True)
    parser.add_argument('--originals',type=P,required=True)
    parser.add_argument('--output',type=P,required=True)
    args=parser.parse_args()
    if os.geteuid()==0: parser.error('Build packages as the regular user')
    base=args.output.resolve();base.mkdir(parents=True,exist_ok=True)
    originals=sorted(args.originals.resolve().glob('*.rpm'));assert originals
    metadata=[]
    for rpm in originals:
        fields=query(rpm,'%{NAME}|%{VERSION}|%{RELEASE}|%{ARCH}|%{LICENSE}|%{SUMMARY}').split('|')
        name,version,release,arch,license,summary=fields
        assert name.split('-')[0] in ['freetype','cairo','harfbuzz','pango'],name
        installed=run(['rpm','-q','--qf','%{VERSION}-%{RELEASE}',name+'.'+arch]).strip()
        assert installed==version+'-'+release,(name,installed)
        metadata.append(dict(name=name,version=version,release=release,arch=arch,license=license,summary=summary,original=str(rpm),original_sha256=sha(rpm)))
    versions={p['version']+'-'+p['release']:p['version']+'-'+p['release']+'.domed.bitmap1' for p in metadata}
    # Fedora's host build policy may omit multilib from compatible *build*
    # architectures. These payloads were already cross-built and executed.
    # Extend only this invocation, never the system RPM configuration.
    rc=base/'multilib-build.rc'
    rc.write_text('buildarch_compat: x86_64_v3: x86_64_v2 x86_64 i686 noarch\nbuildarch_compat: x86_64: i686 noarch\n')
    records=[];abi=[]
    for item in metadata:
        name,version,arch=item['name'],item['version'],item['arch'];release=item['release']+'.domed.bitmap1'
        ident=name+'.'+arch;root=base/'payloads'/ident;root.mkdir(parents=True,exist_ok=True)
        proc=subprocess.Popen(['rpm2cpio',item['original']],stdout=subprocess.PIPE)
        result=subprocess.run(['cpio','-idm','--quiet'],stdin=proc.stdout,cwd=root,capture_output=True);proc.stdout.close();assert proc.wait()==0 and result.returncode==0,result.stderr
        prefix=(args.prefix32 if arch=='i686' else args.prefix).resolve()
        for file in list(root.rglob('*')):
            if not file.is_file() or file.is_symlink():continue
            relative=file.relative_to(root)
            if str(relative).startswith(('usr/lib/','usr/lib64/')) and '.so.' in file.name:
                replacement=prefix/'lib'/file.name
                if replacement.is_file():
                    missing=sorted(exports(file)-exports(replacement));assert not missing,(ident,file.name,missing)
                    abi.append(dict(package=ident,library=file.name,missing_symbols=missing))
                    # Refresh build-id ownership for replaced DSOs; unrelated
                    # tools and documentation keep their original payload.
                    notes=run(['readelf','-n',replacement])
                    match=re.search(r'Build ID: ([0-9a-f]+)',notes)
                    links=root/'usr/lib/.build-id'
                    if links.exists():
                        for link in links.rglob('*'):
                            if link.is_symlink() and link.resolve()==file.resolve():link.unlink()
                    shutil.copy2(replacement,file)
                    if match:
                        ident_hash=match[1];link=links/ident_hash[:2]/ident_hash[2:];link.parent.mkdir(parents=True,exist_ok=True)
                        if not link.exists():link.symlink_to(os.path.relpath(file,link.parent))
                    records.append(dict(path='/'+str(relative),sha256=sha(file)))
            elif str(relative).startswith('usr/include/'):
                replacement=prefix/'include'/str(relative)[len('usr/include/'):]
                if replacement.is_file():shutil.copy2(replacement,file)
        # Preserve distro headers/tools, but add complete source archives, patches and licenses.
        if name=='harfbuzz' and arch=='x86_64':
            notices=root/'usr/share/domed-bdf-runtime';shutil.copytree(args.prefix/'share/domed-bdf-runtime',notices,dirs_exist_ok=True)
        # Runtime sources and manifests must remain readable regardless of the
        # developer's umask. Preserve executable bits for libraries/tools.
        root.chmod(0o755)
        for entry in root.rglob('*'):
            if not entry.is_symlink():entry.chmod(0o755 if entry.is_dir() or entry.stat().st_mode&0o111 else 0o644)
        requires=run(['rpm','-qp','--requires',item['original']]).splitlines()
        provides=run(['rpm','-qp','--provides',item['original']]).splitlines()
        def align(s):
            for old,new in versions.items():s=s.replace(' = '+old,' = '+new)
            return s
        top=base/'rpmbuild'/ident
        for d in ['BUILD','BUILDROOT','RPMS','SOURCES','SPECS','SRPMS']:(top/d).mkdir(parents=True,exist_ok=True)
        lines=[f'Name: {name}',f'Version: {version}',f'Release: {release}',f'Summary: {item["summary"]} (DomeD bitmap policy)',f'License: {item["license"]}',f'BuildArch: {arch}','AutoReqProv: yes']
        lines += ['Requires: '+align(s) for s in requires if not s.startswith('rpmlib(')]
        lines += ['Provides: '+align(s) for s in provides if not s.startswith((name+' =',name+'('))]
        lines += ['%description','Local optimized BDF/PCF font runtime. Original installed package payload and ABI retained.','%prep','%build','%install','mkdir -p %{buildroot}','cp -a '+shlex.quote(str(root))+'/. %{buildroot}/','%files']
        # Only list owned files, avoiding ownership of common parent directories.
        for file in sorted(root.rglob('*')):
            if file.is_dir() and not file.is_symlink():continue
            path='/'+str(file.relative_to(root));assert '\n' not in path
            lines.append('"'+path.replace('%','%%').replace('"','\\"')+'"')
        spec=top/'SPECS'/f'{name}.spec';spec.write_text('\n'.join(lines)+'\n')
        with (base/(ident+'.log')).open('w') as log:
            subprocess.run(['rpmbuild','--rcfile','/usr/lib/rpm/rpmrc:'+str(rc),'-bb','--target',arch,'--define','_topdir '+str(top),'--define','_build_id_links none','--define','__os_install_post %{nil}',str(spec)],stdout=log,stderr=log,check=True)
        outputs=list((top/'RPMS').rglob('*.rpm'));assert len(outputs)==1,outputs
        item['archive']=str(outputs[0]);item['sha256']=sha(outputs[0]);item['new_release']=release
        if name=='harfbuzz' and arch=='x86_64':
            modes=query(outputs[0],'[%{FILENAMES} %{FILEMODES:perms}\n]')
            assert '/usr/share/domed-bdf-runtime/manifest.json -rw-r--r--' in modes,'Runtime manifest must be readable without root'

    plan=dict(format='rpm',packages=metadata,new_libraries=records,abi=abi,prefix=str(args.prefix.resolve()),prefix32=str(args.prefix32.resolve()))
    (base/'plan.json').write_text(json.dumps(plan,indent=2)+'\n')
    # RPM resolves the complete transaction against the real, read-only package database.
    import rpm
    ts=rpm.TransactionSet();ts.setVSFlags(rpm._RPMVSF_NOSIGNATURES)
    for item in metadata:
        fd=os.open(item['archive'],os.O_RDONLY)
        try:header=ts.hdrFromFdno(fd)
        finally:os.close(fd)
        ts.addInstall(header,item['archive'],'u')
    problems=ts.check();(base/'dependency-check.json').write_text(json.dumps(problems,default=str,indent=2)+'\n')
    assert not problems,problems
    print('Staged',len(metadata),'RPMs; ABI and complete dependency transaction passed')
if __name__=='__main__':main()
