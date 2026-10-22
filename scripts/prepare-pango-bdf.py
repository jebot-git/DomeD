#!/usr/bin/env python3
"""Build private Pango with raw BDF metrics; never install distribution libraries."""
import argparse,hashlib,pathlib,shutil,subprocess,tarfile,urllib.request
root=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--prefix',type=pathlib.Path,default=root.parent/'fltk-experiment/_toolkit');args=p.parse_args()
version='1.57.0';digest='890640c841dae77d3ae3d8fe8953784b930fa241b17423e6120c7bfdf8b891e7'
cache=root.parent/'fltk-experiment/_deps/pango-bdf';cache.mkdir(parents=True,exist_ok=True)
archive=cache/('pango-'+version+'.tar.xz');url='https://download.gnome.org/sources/pango/1.57/'+archive.name
if not archive.exists():archive.write_bytes(urllib.request.urlopen(url,timeout=120).read())
assert hashlib.sha256(archive.read_bytes()).hexdigest()==digest,'Pango archive checksum mismatch'
patch=root/'native/toolkit-hooks/pango-1.57.0-native-bdf.patch';patch_hash=hashlib.sha256(patch.read_bytes()).hexdigest();source=cache/('pango-'+version+'-'+patch_hash[:12]);build=cache/('build-'+patch_hash[:12])
if not source.exists():
 temp=cache/'unpack';temp.mkdir(exist_ok=True)
 with tarfile.open(archive) as tar:tar.extractall(temp,filter='data')
 (temp/('pango-'+version)).rename(source)
 subprocess.run(['patch','-p1','--input',str(patch)],cwd=source,check=True)
command=['meson','setup',str(build),str(source),'--prefix='+str(args.prefix.resolve()),'--libdir=lib','-Dbuild-testsuite=false','-Dbuild-examples=false','-Dintrospection=disabled','-Dlibthai=disabled','-Dxft=disabled']
if (build/'build.ninja').exists():command.append('--reconfigure')
subprocess.run(command,check=True);subprocess.run(['ninja','-C',str(build),'-j2'],check=True);subprocess.run(['meson','install','-C',str(build)],check=True)
notices=args.prefix/'share/pango-bdf-source';notices.mkdir(parents=True,exist_ok=True)
for file in [archive,patch,source/'COPYING']:shutil.copy2(file,notices/file.name)
print('Installed private raw-BDF Pango in',args.prefix)
