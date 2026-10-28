#!/usr/bin/env python3
"""Change only RapidOCR's OpenCV dependency; preserve the upstream Python code."""
import argparse,base64,csv,hashlib,io,re,zipfile
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('upstream',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
with zipfile.ZipFile(a.upstream) as z:
 metadata=next(name for name in z.namelist() if name.endswith('.dist-info/METADATA'))
 info=metadata.rsplit('/',1)[0];text=z.read(metadata).decode()
 version=re.search(r'^Version: (.+)$',text,re.M)[1]
 new=version+'+domed1';newinfo=info.replace(version,new)
 text=text.replace('Version: '+version+'\n','Version: '+new+'\n')
 text,count=re.subn(r'(?im)^(Requires-Dist: )opencv[-_]python\b',r'\1opencv-python-headless',text)
 assert count==1,'Unexpected RapidOCR dependency metadata'
 files={name.replace(info+'/',newinfo+'/'):z.read(name) for name in z.namelist() if not name.endswith('/RECORD')}
 files[newinfo+'/METADATA']=text.encode()
 stream=io.StringIO();writer=csv.writer(stream,lineterminator='\n')
 for name,data in sorted(files.items()):
  digest=base64.urlsafe_b64encode(hashlib.sha256(data).digest()).decode().rstrip('=')
  writer.writerow([name,'sha256='+digest,len(data)])
 writer.writerow([newinfo+'/RECORD','','']);files[newinfo+'/RECORD']=stream.getvalue().encode()
 with zipfile.ZipFile(a.output,'w',zipfile.ZIP_DEFLATED) as output:
  for name,data in files.items():
   item=zipfile.ZipInfo(name,date_time=(2026,10,28,0,0,0));item.compress_type=zipfile.ZIP_DEFLATED;item.external_attr=0o644<<16
   output.writestr(item,data)
 with zipfile.ZipFile(a.output) as rebuilt:
  for name in z.namelist():
   if not '.dist-info/' in name:assert rebuilt.read(name)==z.read(name)
print('Verified unchanged upstream payload; dependency is now opencv-python-headless.')
