#!/usr/bin/env python3
from pathlib import Path
import argparse,shutil,subprocess
p=argparse.ArgumentParser(description='Package verified Fcitx/SKK staging trees without toolkit frontends')
p.add_argument('--work',type=Path,required=True);a=p.parse_args()
root=a.work.resolve()
rpmbuild=root/'rpm';(rpmbuild/'SPECS').mkdir(parents=True,exist_ok=True)
stage=root/'fcitx-stage'
shutil.copy2(Path(__file__).with_name('fcitx5-configtool'),stage/'usr/bin/fcitx5-configtool')
(stage/'usr/bin/fcitx5-configtool').chmod(0o755)
for name in ('/etc/X11/xinit/xinput.d/fcitx5.conf','/etc/xdg/Xwayland-session.d/20-fcitx-x11'):
 p=Path(name)
 if p.is_file():
  dest=stage/name.lstrip('/');dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,dest)
main=[];libs=[];data=[]
for p in stage.rglob('*'):
 if p.is_dir():continue
 name='/'+str(p.relative_to(stage))
 if name.startswith(('/usr/include/','/usr/lib64/cmake/','/usr/lib64/pkgconfig/')):continue
 if '/testing/' in name or '/libtest' in name or name.endswith('.a'):continue
 if name.startswith('/usr/lib64/libFcitx5'):
  if '.so.' in name:libs.append(name)
 elif name.startswith('/usr/share/'):data.append(name)
 else:main.append(name)
for name,paths in [('main',main),('libs',libs),('data',data)]:
 (rpmbuild/f'{name}.files').write_text('\n'.join(sorted(paths))+'\n')
spec=f'''%global _build_id_links none
%global debug_package %{{nil}}
Name: fcitx5
Version: 5.1.23
Release: 2.fc44.domed1
Summary: Native Wayland Fcitx input service with FLTK configuration
License: LGPL-2.1-or-later
URL: https://github.com/fcitx/fcitx5
Requires: fcitx5-libs%{{?_isa}} = %{{version}}-%{{release}}
Requires: fcitx5-data = %{{version}}-%{{release}}
%description
Optimized upstream Fcitx 5.1.23 build. No Qt or GTK libraries are used by
its service or classic candidate renderer. Configuration uses DomeD FLTK.
%package libs
Summary: Toolkit-independent Fcitx runtime libraries
%description libs
Fcitx utility, configuration and input-method runtime libraries.
%package data
Summary: Fcitx language data and native Wayland candidate themes
BuildArch: noarch
%description data
Input method metadata, themes, locale data and keyboard definitions.
%prep
%build
%install
mkdir -p %{{buildroot}}
cp -a {stage}/. %{{buildroot}}/
rm -rf %{{buildroot}}/usr/include %{{buildroot}}/usr/lib64/cmake %{{buildroot}}/usr/lib64/pkgconfig
find %{{buildroot}} -name '*.a' -delete
find %{{buildroot}}/usr/lib64 -maxdepth 1 -name 'libFcitx5*.so' -delete
rm -rf %{{buildroot}}/usr/share/fcitx5/testing
find %{{buildroot}}/usr/lib64/fcitx5 -name 'libtest*' -delete
%files -f {rpmbuild}/main.files
%files libs -f {rpmbuild}/libs.files
%files data -f {rpmbuild}/data.files
'''
(rpmbuild/'SPECS/fcitx5.spec').write_text(spec)
subprocess.run(['rpmbuild','-bb','--define',f'_topdir {rpmbuild}',str(rpmbuild/'SPECS/fcitx5.spec')],check=True)
stage=root/'skk-stage'
paths=['/'+str(p.relative_to(stage)) for p in stage.rglob('*') if not p.is_dir()]
(rpmbuild/'skk.files').write_text('\n'.join(sorted(paths))+'\n')
spec=f'''%global _build_id_links none
%global debug_package %{{nil}}
Name: fcitx5-skk
Version: 5.1.10
Release: 2.fc44.domed1
Summary: SKK Japanese input engine without the optional Qt editor
License: GPL-2.0-or-later
URL: https://github.com/fcitx/fcitx5-skk
Requires: fcitx5-data
Requires: skkdic
%description
Upstream SKK engine built with ENABLE_QT=OFF. The engine and dictionary
support remain available without GTK or Qt configuration libraries.
%prep
%build
%install
mkdir -p %{{buildroot}}
cp -a {stage}/. %{{buildroot}}/
%files -f {rpmbuild}/skk.files
'''
(rpmbuild/'SPECS/fcitx5-skk.spec').write_text(spec)
subprocess.run(['rpmbuild','-bb','--define',f'_topdir {rpmbuild}',str(rpmbuild/'SPECS/fcitx5-skk.spec')],check=True)
