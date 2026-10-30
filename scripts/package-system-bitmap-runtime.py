#!/usr/bin/python3
"""Package the tested BDF runtime as version-aligned system Debian packages."""
from pathlib import Path
import subprocess,json,shutil,re,hashlib,gzip,argparse,apt
repo=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--prefix',type=Path,default=repo/'build/bitmap-runtime')
parser.add_argument('--output-dir',type=Path,default=repo/'build/system-bdf-packages')
parser.add_argument('--original-archives',type=Path,help='Optional directory of installed-version .deb archives')
args=parser.parse_args();base=args.output_dir.resolve();base.mkdir(parents=True,exist_ok=True)
prefix=args.prefix.resolve();cache=apt.Cache()
packages=[dict(name=p.name,version=p.installed.version,source=p.installed.source_name,manual=not p.is_auto_installed) for p in cache if p.is_installed and p.installed.source_name in ['harfbuzz','pango1.0']]
if len(packages)!=14:raise SystemExit('Expected seven installed HarfBuzz and seven Pango packages')
(base/'original-packages.json').write_text(json.dumps(packages,indent=2)+'\n')
rollback=base/'rollback-packages';rollback.mkdir(exist_ok=True)
for item in packages:
    matches=list(args.original_archives.glob(item['name']+'_'+item['version']+'_*.deb')) if args.original_archives else []
    if matches and matches[0].resolve()!=(rollback/matches[0].name).resolve():shutil.copy2(matches[0],rollback/matches[0].name)
missing=[p for p in packages if not list(rollback.glob(p['name']+'_'+p['version']+'_*.deb'))]
if missing:subprocess.run(['apt-get','download',*[p['name']+'='+p['version'] for p in missing]],cwd=rollback,check=True)
versions={'harfbuzz':'12.3.2-2+domed.bdf.20261030','pango1.0':'1.57.0-1+domed.bdf.20261030.2'}
replace={'libharfbuzz0b':'libharfbuzz.so.0.61232.0','libharfbuzz-subset0':'libharfbuzz-subset.so.0.61232.0','libpango-1.0-0':'libpango-1.0.so.0.5700.0','libpangocairo-1.0-0':'libpangocairo-1.0.so.0.5700.0','libpangoft2-1.0-0':'libpangoft2-1.0.so.0.5700.0'}
manifest=json.loads((prefix/'share/domed-bdf-runtime/manifest.json').read_text())
assert manifest['policy']=='native-unicode-bdf-only'
for name,h in manifest['libraries'].items():assert hashlib.sha256((prefix/'lib'/name).read_bytes()).hexdigest()==h
output=base/'packages';output.mkdir(exist_ok=True);checks=[];libs=[]
def exported(path):
 s=subprocess.check_output(['nm','-D','--defined-only',str(path)],text=True)
 return {line.split()[-1] for line in s.splitlines() if len(line.split())>=3}
for item in packages:
 name=item['name'];version=versions[item['source']]
 original=next((base/'rollback-packages').glob(name+'_'+item['version']+'_*.deb'))
 tree=base/'package-trees'/name;tree.parent.mkdir(exist_ok=True)
 if tree.exists():shutil.rmtree(tree)
 subprocess.run(['dpkg-deb','--raw-extract',str(original),str(tree)],check=True)
 if name in replace:
  filename=replace[name];new=prefix/'lib'/filename;old=Path('/usr/lib/x86_64-linux-gnu')/filename
  missing=sorted(exported(old)-exported(new));checks.append(dict(library=filename,missing_symbols=missing));assert set(missing) <= ({'hb_graphite2_face_get_gr_face','hb_graphite2_font_get_gr_font'} if name=='libharfbuzz0b' else set()),(filename,missing)
  shutil.copy2(new,tree/'usr/lib/x86_64-linux-gnu'/filename)
  libs.append(dict(path='/usr/lib/x86_64-linux-gnu/'+filename,sha256=hashlib.sha256(new.read_bytes()).hexdigest()))
 if name=='libharfbuzz0b':
  notices=tree/'usr/share/domed-bdf-runtime'
  if notices.exists():shutil.rmtree(notices)
  shutil.copytree(prefix/'share/domed-bdf-runtime',notices)
  (notices/'manifest.json').write_text(json.dumps({**manifest,'deployment':'system','package_versions':versions},indent=2)+'\n')
  pin=tree/'etc/apt/preferences.d/90-domed-bdf-runtime';pin.parent.mkdir(parents=True,exist_ok=True)
  shutil.copy2(repo/'packaging/90-domed-bdf-runtime.pref',pin)
  (tree/'DEBIAN/conffiles').write_text('/etc/apt/preferences.d/90-domed-bdf-runtime\n')
  checker=tree/'usr/local/libexec/domed/check-bitmap-runtime';checker.parent.mkdir(parents=True,exist_ok=True)
  shutil.copy2(repo/'scripts/check-bitmap-runtime.py',checker);checker.chmod(0o755)
 control=tree/'DEBIAN/control';text=control.read_text();text=re.sub(r'^Version: .*$', 'Version: '+version,text,flags=re.M)
 for related in packages:
  text=re.sub(r'('+re.escape(related['name'])+r'(?:\:[a-z0-9-]+)?\s*\(=\s*)'+re.escape(related['version'])+r'(\))',lambda m:m[1]+versions[related['source']]+m[2],text)
 text=re.sub(r'^Maintainer: .*$', 'Maintainer: Local DomeD Bitmap Fork <root@localhost>',text,flags=re.M)
 docs=tree/'usr/share/doc'/name;docs.mkdir(parents=True,exist_ok=True)
 with gzip.open(docs/'changelog.DomeD.gz','wb') as f:f.write(('Local BDF-only HarfBuzz fork and paired Pango; native Unicode BDF only.\nRequired system dependencies retained; outline fonts intentionally unsupported.\nVersion '+version+'\n').encode())
 md5=[];size=0
 for f in sorted(tree.rglob('*')):
  if f.is_file() and not f.is_symlink() and not f.is_relative_to(tree/'DEBIAN'):
   data=f.read_bytes();size+=len(data);md5.append(hashlib.md5(data).hexdigest()+'  '+str(f.relative_to(tree)))
 (tree/'DEBIAN/md5sums').write_text('\n'.join(md5)+'\n')
 text=re.sub(r'^Installed-Size: .*$', 'Installed-Size: '+str((size+1023)//1024),text,flags=re.M);control.write_text(text)
 # Package public runtime data with system-readable modes, independent of umask.
 tree.chmod(0o755)
 for entry in tree.rglob('*'):
  if entry.is_symlink():continue
  entry.chmod(0o755 if entry.is_dir() or entry.stat().st_mode & 0o111 else 0o644)
 subprocess.run(['dpkg-deb','--root-owner-group','--build',str(tree),str(output/(name+'_'+version+'_amd64.deb'))],check=True,stdout=subprocess.DEVNULL)
(base/'abi-comparison.json').write_text(json.dumps(checks,indent=2)+'\n')
archives=[dict(path=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sorted(output.glob('*.deb'))]
r=subprocess.run(['apt-get','-s','--reinstall','install',*[a['path'] for a in archives]],text=True,capture_output=True)
(base/'transaction-plan.log').write_text(r.stdout+r.stderr);assert r.returncode==0,r.stdout+r.stderr
assert not re.findall(r'^(?:Remv|Purg) (\S+)',r.stdout,re.M)
assert set(re.findall(r'^Inst (\S+)',r.stdout,re.M))=={p['name'] for p in packages}
(base/'plan.json').write_text(json.dumps(dict(versions=versions,packages=packages,archives=archives,new_libraries=libs,manifest=manifest),indent=2)+'\n')
print('Fourteen packages prepared; five replacement DSOs audited (Graphite outline backend intentionally omitted); APT plans no removals.')
