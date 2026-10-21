#!/usr/bin/env python3
"""Stage the reviewed DomeD, WPE and FLTube release patches without root."""
import hashlib,json,os,shutil,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
WORKSPACE=ROOT.parent
STAGE=ROOT/'build/workspace-patch-stage'
STAGE.mkdir(parents=True,exist_ok=True)
subprocess.run([str(ROOT/'scripts/stage-tray.sh'),str(STAGE)],check=True)
def copy(source,target,mode=0o644):
    destination=STAGE/target;destination.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(source,destination);destination.chmod(mode);return destination
copy(ROOT/'target/release/dome-compositor','opt/fltk-vr/bin/dome-compositor',0o755)
copy(WORKSPACE/'FLtube/build-system-release/fltube','usr/local/bin/fltube',0o755)
copy(WORKSPACE/'FLtube/README.md','usr/local/share/doc/fltube/README.md')
wpe=WORKSPACE/'fltk-wpe-xr'
binary=copy(wpe/'fltk-wpe/build/fltk-wpe','opt/fltk-wpe-xr/fltk-wpe/build/fltk-wpe',0o755)
subprocess.run(['patchelf','--set-rpath','$ORIGIN/../../_deps/nightly/lib:$ORIGIN/../../_deps/nightly/sys/lib:/opt/fltk-vr/toolkit/lib',str(binary)],check=True)
copy(wpe/'fltk-wpe/GPU_PRESENTATION.md','opt/fltk-wpe-xr/share/doc/GPU_PRESENTATION.md')
# Preserve the complete runtime inventory when applying a binary-only update.
receipt=Path('/opt/fltk-wpe-xr/install-manifest.json')
old=json.loads(receipt.read_text())
updates={}
for relative in ['opt/fltk-wpe-xr/fltk-wpe/build/fltk-wpe','opt/fltk-wpe-xr/share/doc/GPU_PRESENTATION.md']:
    p=STAGE/relative;updates[relative]={'path':relative,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'mode':p.stat().st_mode&0o777}
merged={item['path']:item for item in old};merged.update(updates)
p=STAGE/'opt/fltk-wpe-xr/install-manifest.json';p.write_text(json.dumps(list(merged.values()),indent=2)+'\n');p.chmod(0o644)
files=[]
for p in sorted(STAGE.rglob('*')):
    if p.is_file() and p.name!='patch-manifest.json':files.append({'path':str(p.relative_to(STAGE)),'mode':p.stat().st_mode&0o777,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()})
(STAGE/'patch-manifest.json').write_text(json.dumps(files,indent=2)+'\n')
print('Prepared',len(files),'verified patch files at',STAGE)
