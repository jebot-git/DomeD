#!/usr/bin/env python3
"""Build and stage native Fcitx/SKK as the regular user; never install system files."""
from pathlib import Path
import argparse,os,subprocess
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--source',type=Path,required=True);p.add_argument('--skk-source',type=Path,required=True)
p.add_argument('--work',type=Path,required=True);p.add_argument('--deps-root',type=Path)
a=p.parse_args()
if os.geteuid()==0:raise SystemExit('Build as the regular user.')
work=a.work.resolve();work.mkdir(parents=True,exist_ok=True)
source=a.source.resolve();skk=a.skk_source.resolve()
for path,revision in ((source,'27cca6e0239938b2d53937061d0980c087856b22'),(skk,'176361f7711d7b1e9f98f08eb94f750474aeba3c')):
 actual=subprocess.check_output(['git','-C',str(path),'rev-parse','HEAD'],text=True).strip()
 if not actual.startswith(revision):raise SystemExit('Unexpected source revision: '+str(path))
env=dict(os.environ);prefix=[]
if a.deps_root:
 deps=a.deps_root.resolve()/'usr';prefix=['-DCMAKE_PREFIX_PATH='+str(deps)]
 env['PKG_CONFIG_PATH']=':'.join([str(deps/'lib64/pkgconfig'),str(deps/'share/pkgconfig'),env.get('PKG_CONFIG_PATH','')])
def run(args,**kwargs):subprocess.run([str(x) for x in args],check=True,env=env,**kwargs)
common=['-G','Ninja','-DCMAKE_BUILD_TYPE=Release','-DCMAKE_INSTALL_PREFIX=/usr','-DCMAKE_INSTALL_LIBDIR=lib64',*prefix]
run(['cmake','-S',source,'-B',work/'fcitx-build',*common,'-DENABLE_TEST=ON','-DENABLE_DOC=OFF','-DUSE_SYSTEM_YOGA=OFF'])
run(['cmake','--build',work/'fcitx-build','-j4'])
run(['ctest','--test-dir',work/'fcitx-build','--output-on-failure'])
subprocess.run(['cmake','--install',str(work/'fcitx-build')],check=True,env={**env,'DESTDIR':str(work/'fcitx-stage')})
run(['cmake','-S',skk,'-B',work/'skk-build',*common,'-DENABLE_QT=OFF','-DCMAKE_SKIP_INSTALL_RPATH=ON'])
run(['cmake','--build',work/'skk-build','-j4'])
subprocess.run(['cmake','--install',str(work/'skk-build')],check=True,env={**env,'DESTDIR':str(work/'skk-stage')})
print('Built and staged. Review package requirements before using sudo for installation.')
