#!/usr/bin/env python3
"""Index verified bundled BDF metadata so opening the picker does not load all glyphs.
Run after importing fonts. Fontconfig and FreeType still validate user-installed fonts.
"""
import hashlib,json,pathlib,subprocess
root=pathlib.Path(__file__).resolve().parents[1];assets=root/'assets/fonts'
paths=sorted(p for p in assets.rglob('*.bdf') if p.name!='upstream.bdf')
result=subprocess.run(['fc-scan','--format','%{file}\t%{fontformat}\t%{family[0]}\t%{style[0]}\t%{pixelsize}\t%{charset}\n',*[str(p) for p in paths]],capture_output=True,text=True,check=True)
records={}
for row in result.stdout.splitlines():
 file,format,family,style,pixels,charset=row.split('\t');assert format=='BDF',file
 text=any(int(r.split('-')[0],16)<=65<=int(r.split('-')[-1],16) for r in charset.split())
 p=pathlib.Path(file);relative=p.relative_to(assets).as_posix();assert all('\t' not in s and '\n' not in s for s in [relative,family,style])
 records[relative]='\t'.join([relative,family,style,str(round(float(pixels))),'1' if text else '0',hashlib.sha256(p.read_bytes()).hexdigest()])
assert len(records)==len(paths),(len(records),len(paths))
(assets/'fonts.index').write_text('\n'.join(records[k] for k in sorted(records))+'\n')
(root/'packaging/font-notices.list').write_text(''.join(p.relative_to(assets).as_posix()+'\n' for p in sorted(assets.rglob('*')) if p.is_file() and p.suffix!='.bdf'))
print(len(records),'BDF files indexed')
