#!/usr/bin/env python3
"""Import a pinned bitmap source directory, preserving pixels and mapping legacy encodings.

Requires fonttools. This is a packaging tool; no format conversion runs in DomeD.
Usage: import-bdf-collection.py SOURCE DESTINATION REPOSITORY REVISION [GLOB]
"""
import hashlib,json,pathlib,re,sys
from fontTools import agl
src,dest=map(pathlib.Path,sys.argv[1:3]);dest.mkdir(parents=True,exist_ok=True)
repo,revision=sys.argv[3:5];pattern=sys.argv[5] if len(sys.argv)>5 else '**/*'
records={};files={};skipped=[]
def normalize(data):
 text=data.decode('utf-8',errors='replace').replace('\r\n','\n')
 head,*glyphs=text.split('STARTCHAR ')
 head='\n'.join(line.lstrip().encode('ascii','backslashreplace').decode() if line.lstrip().startswith('COMMENT') else line.lstrip() for line in head.splitlines())+'\n'
 def prop(key,default=''):
  m=re.search(r'(?m)^\s*'+key+r'\s+"?([^"\n]+)',head);return m[1].strip() if m else default
 font=prop('FONT');parts=font.split('-');reg=prop('CHARSET_REGISTRY',parts[-2] if len(parts)>=15 else 'ISO10646');enc=prop('CHARSET_ENCODING',parts[-1] if len(parts)>=15 else '1')
 key=reg.upper();codec=None
 if key=='MACROMAN':codec='mac_roman'
 elif key in ('IBM','MICROSOFT'):codec='cp'+enc.lower().removeprefix('cp')
 elif key=='WINROMAN':codec='cp1252'
 elif key=='ISO8859':codec='iso8859_'+enc
 elif key in ('US','ISO646.1991'):codec='ascii'
 elif key.startswith(('KSX1001','KSC5601')):codec='euc_kr'
 elif key.startswith('JISX0208'):codec='euc_jp'
 elif key.startswith('JISX0213'):codec='euc_jis_2004'
 unicode=key in ('ISO10646','UNICODE');out=[];used=set();changed=not unicode
 for glyph in glyphs:
  match=re.search(r'(?m)^ENCODING\s+(-?\d+)',glyph)
  if not match:continue
  n=int(match[1]);cp=n
  if n<0:continue
  if key.startswith("JOHAB"):cp=0x100000+n
  elif codec:
   try:
    raw=(bytes([n>>8|128,n&255|128]) if codec.startswith('euc') else bytes([n]))
    if codec=='euc_jis_2004' and enc=='2':raw=b'\x8f'+raw
    value=raw.decode(codec);cp=ord(value) if len(value)==1 else -1
   except (ValueError,UnicodeError):cp=-1
  elif not unicode:
   name=glyph.splitlines()[0].strip();value=agl.toUnicode(name)
   if len(value)==1:cp=ord(value)
   elif re.fullmatch(r'(?:U\+|uni|u)[0-9A-Fa-f]{4,6}',name):cp=int(re.sub(r'^(U\+|uni|u)','',name),16)
   elif n<128:cp=n
   else:cp=-1
  if cp<0 or cp>0x10ffff or cp in used:continue
  used.add(cp);glyph=re.sub(r'(?m)^ENCODING\s+[^\n]+','ENCODING '+str(cp),glyph)
  out.append('STARTCHAR '+glyph.split('ENDFONT')[0])
 if not out:raise ValueError('No mapped glyphs')
 # Explicit metadata makes legacy Macintosh and fonts without properties usable by Unicode toolkits.
 props={}
 if changed or not prop('CHARSET_REGISTRY'):props.update(CHARSET_REGISTRY='"ISO10646"',CHARSET_ENCODING='"1"')
 if not prop('FAMILY_NAME'):props['FAMILY_NAME']='"'+(parts[2] if len(parts)>=15 else font or src.name)+'"'
 if not prop('PIXEL_SIZE'):
  size=prop('SIZE','16 75 75').split();props['PIXEL_SIZE']=str(round(float(size[0])*float(size[2])/72))
 for k,v in props.items():
  if re.search(r'(?m)^\s*'+k+r'\s+',head):head=re.sub(r'(?m)^\s*'+k+r'\s+[^\n]+',k+' '+v,head)
  else:head=head.replace('ENDPROPERTIES',k+' '+v+'\nENDPROPERTIES')
 if props:
  start=head.index('STARTPROPERTIES');end=head.index('ENDPROPERTIES');count=len(head[start:end].splitlines())-1
  head=re.sub(r'STARTPROPERTIES\s+\d+','STARTPROPERTIES '+str(count),head)
 if changed and len(parts)>=15:
  parts[-2:]=['ISO10646','1'];head=re.sub(r'(?m)^FONT\s+[^\n]+','FONT '+'-'.join(parts),head)
 head=re.sub(r'(?m)^CHARS\s+\d+','CHARS '+str(len(out)),head)
 if changed:head=head.replace('STARTPROPERTIES','COMMENT DomeD packaging: legacy character codes mapped to Unicode; glyph pixels unchanged.\nSTARTPROPERTIES',1)
 return (head+''.join(out)+'ENDFONT\n').encode(),reg+'-'+enc,changed
for p in sorted(src.glob(pattern)):
 if not p.is_file() or p.suffix.lower()!='.bdf':continue
 try:
  original=p.read_bytes();data,encoding,changed=normalize(original)
  relative=p.relative_to(src).with_suffix('.bdf');target=dest/relative;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(data)
  name=relative.as_posix();files[name]=hashlib.sha256(data).hexdigest();records[name]={'source_sha256':hashlib.sha256(original).hexdigest(),'encoding':encoding,'unicode_mapping':changed}
 except Exception as e:skipped.append({'file':str(p.relative_to(src)),'reason':str(e)})
# Preserve accompanying terms and source documentation, including nested per-font notices.
for p in sorted(src.rglob('*')):
 if not p.is_file() or not any(t in p.name.lower() for t in ('license','copying','copyright','readme','authors','ofl.txt')):continue
 if p.suffix.lower() in ('.bdf','.glyph','.bitmap'):continue
 relative=p.relative_to(src);target=dest/relative;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(p.read_bytes());files[relative.as_posix()]=hashlib.sha256(p.read_bytes()).hexdigest()
manifest={'repository':repo,'revision':revision,'format':'BDF only; original bitmap pixels; legacy character codes mapped to Unicode for session text','files':files,'fonts':records,'skipped':skipped}
(dest/'upstream.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
print(dest,len(records),'BDF fonts;',len(skipped),'skipped')
