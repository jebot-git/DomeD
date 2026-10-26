#!/usr/bin/env python3
"""Build the pinned bundled de-xr experiences as the regular user, then stage them."""
import argparse,json,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('stage',type=Path);p.add_argument('--source-root',type=Path);a=p.parse_args()
root=Path(__file__).resolve().parents[1];stage=a.stage.resolve()
if os.geteuid()==0:raise SystemExit('Build bundled experiences as the regular user; sudo is only for installation.')
if stage==Path('/'):raise SystemExit('Use a staging directory, not the live root filesystem.')
manifest=json.loads((root/'packaging/experiences.json').read_text())
if manifest.get('version')!=1:raise SystemExit('Unsupported bundled-experience manifest version')
for experience in manifest['experiences']:
    revision=experience['revision'];name=experience['source_directory']
    if len(revision)!=40 or any(c not in '0123456789abcdef' for c in revision):raise SystemExit('Invalid pinned experience revision')
    candidate=(a.source_root or root.parent)/name
    def git(path,*args):return subprocess.check_output(['git','-C',str(path),*args],text=True).strip()
    source=None
    if (candidate/'.git').exists():
        if git(candidate,'rev-parse','HEAD')==revision and not git(candidate,'status','--porcelain'):
            source=candidate
    if source is None:
        source=root/'build/experiences/sources'/(name+'-'+revision)
        if not source.exists():
            source.parent.mkdir(parents=True,exist_ok=True)
            local=(candidate/'.git').exists() and subprocess.run(['git','-C',str(candidate),'cat-file','-e',revision+'^{commit}'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
            if local:
                subprocess.run(['git','-C',str(candidate),'worktree','add','--detach',str(source),revision],check=True)
            else:
                subprocess.run(['git','clone','--no-checkout',experience['repository'],str(source)],check=True)
                subprocess.run(['git','-C',str(source),'checkout','--detach',revision],check=True)
        if git(source,'rev-parse','HEAD')!=revision or git(source,'status','--porcelain'):
            raise SystemExit('Pinned cache has local changes; preserved without reset: '+str(source))
    build=root/'build/experiences'/('build-'+name+'-'+revision)
    subprocess.run(['cmake','-S',str(source),'-B',str(build),'-G','Ninja','-DCMAKE_BUILD_TYPE=Release',
                    '-DCMAKE_INSTALL_PREFIX=/usr/local','-DTEMPEST_OPENXR=ON','-DBUILD_TESTING=OFF'],check=True)
    subprocess.run(['cmake','--build',str(build),'--parallel','4'],check=True)
    subprocess.run(['cmake','--install',str(build)],env={**os.environ,'DESTDIR':str(stage)},check=True)
    desktop=stage/'usr/local/share/applications'/experience['desktop_id']
    subprocess.run(['desktop-file-validate',str(desktop)],check=True)
    version=stage/'usr/local/share/doc'/name.lower()/'BUILD.txt'
    version.parent.mkdir(parents=True,exist_ok=True)
    version.write_text(experience['name']+' '+revision+'\nRelease; native OpenXR; DomeD API '+str(experience['native_xr_api'])+'\n')
    version.chmod(0o644)
print('Staged pinned bundled experiences:',stage)
catalog=stage/'usr/local/share/domed/experiences.json'
catalog.parent.mkdir(parents=True,exist_ok=True)
catalog.write_text(json.dumps(manifest,indent=2)+'\n');catalog.chmod(0o644)
