#!/usr/bin/env python3
"""Actual device-owned SHM teardown, with independent old/new pixel equality."""
import argparse,hashlib,json,os,re,shlex,subprocess,time,traceback
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--prefix',type=Path,required=True);p.add_argument('--reference-prefix',type=Path);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
root=Path(__file__).resolve().parents[1];out=a.output.resolve();out.mkdir(parents=True,exist_ok=True);prefix=a.prefix.resolve()
env=dict(os.environ,PKG_CONFIG_PATH=str(prefix/'lib/pkgconfig')+':'+os.environ.get('PKG_CONFIG_PATH',''),ASAN_OPTIONS='detect_leaks=1:halt_on_error=1',UBSAN_OPTIONS='halt_on_error=1:print_stacktrace=1')
flags=shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','cairo','x11','xext'],env=env,text=True));cmd=['cc','-std=c11','-O1','-g1','-fsanitize=address,undefined','-fno-omit-frame-pointer',str(root/'native/bitmap-runtime/xlib-shm-lifetime.c'),*flags,'-Wl,-rpath,'+str(prefix/'lib'),'-o',str(out/'probe')];subprocess.run(cmd,env=env,check=True)
paths=[root/'native/bitmap-runtime/xlib-shm-lifetime.c',Path(__file__).resolve(),out/'probe']
for runtime in [prefix]+([a.reference_prefix.resolve()] if a.reference_prefix else []):paths.extend(p for p in (runtime/'lib').glob('*.so*') if p.is_file())
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();before={str(p):sha(p) for p in sorted(set(paths))};(out/'before.json').write_text(json.dumps(before,indent=2)+'\n');s={'state':'running','pid':os.getpid(),'runs':[],'sanitizer_suppressions':False,'scope':'Actual public Cairo similar-image API/device-owned SHM paints, independent Xlib pixel hashes and repeated finish/close. Sanitized client with optimized external libraries; private Xvfb, no physical session.'};(out/'status.json').write_text(json.dumps(s,indent=2)+'\n');server=None
try:
 rd,wr=os.pipe();log=(out/'xvfb.log').open('w');server=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','640x640x24','-nolisten','tcp'],pass_fds=(wr,),stdout=log,stderr=log);os.close(wr)
 with os.fdopen(rd) as stream:env['DISPLAY']=':'+stream.readline().strip()
 old_pixels=None
 for label,runtime,expected in ([('old',a.reference_prefix.resolve(),1)] if a.reference_prefix else [])+[('fixed',prefix,0)]:
  start=time.monotonic()
  with (out/(label+'.log')).open('w') as log:result=subprocess.run([str(out/'probe')],env={**env,'LD_LIBRARY_PATH':str(runtime/'lib')},stdout=log,stderr=subprocess.STDOUT,timeout=120)
  text=(out/(label+'.log')).read_text();pixels=re.findall(r'^PIXELS .+$',text,re.M);s['runs'].append({'name':label,'exit':result.returncode,'expected':expected,'pixels':pixels,'seconds':time.monotonic()-start});(out/'status.json').write_text(json.dumps(s,indent=2)+'\n')
  assert result.returncode==expected and len(pixels)==4 and 'runtime error:' not in text
  if label=='old':assert 'LeakSanitizer:' in text and '_cairo_xlib_shm_surface_create' in text;old_pixels=pixels
  else:
   assert 'LeakSanitizer:' not in text and 'PASS four actual deferred-SHM display lifetimes' in text
   if old_pixels is not None:assert pixels==old_pixels
  print(label,result.returncode,pixels,flush=True)
 s.update(exit_code=0,pixels_exact=True)
except BaseException:s.update(exit_code=1,failure=traceback.format_exc());raise
finally:
 if server is not None:server.terminate();server.wait(timeout=5)
 after={p:sha(Path(p)) for p in before};s.update(state='terminal',inputs=len(before),drift=[p for p in before if before[p]!=after[p]]);(out/'after.json').write_text(json.dumps(after,indent=2)+'\n');(out/'status.json').write_text(json.dumps(s,indent=2)+'\n')
