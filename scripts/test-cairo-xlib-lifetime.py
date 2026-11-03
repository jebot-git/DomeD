#!/usr/bin/env python3
"""Actual private Xlib source-cache teardown and unchanged pixel regression."""
import argparse,hashlib,json,os,re,shlex,subprocess,time,traceback
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--prefix',type=Path,required=True)
p.add_argument('--reference-prefix',type=Path)
p.add_argument('--output',type=Path,required=True)
a=p.parse_args();root=Path(__file__).resolve().parents[1];out=a.output.resolve();out.mkdir(parents=True,exist_ok=True);prefix=a.prefix.resolve()
env=os.environ.copy();env['PKG_CONFIG_PATH']=str(prefix/'lib/pkgconfig')+':'+env.get('PKG_CONFIG_PATH','');env.update(ASAN_OPTIONS='detect_leaks=1:halt_on_error=1',UBSAN_OPTIONS='halt_on_error=1')
flags=shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','cairo','freetype2','x11','xrender'],env=env,text=True))
cmd=['cc','-std=c11','-O1','-g1','-fsanitize=address,undefined','-fno-omit-frame-pointer',str(root/'native/bitmap-runtime/xlib-lifetime.c'),*flags,'-Wl,-rpath,'+str(prefix/'lib'),'-o',str(out/'probe')]
subprocess.run(cmd,env=env,check=True)
inputs=[root/'native/bitmap-runtime/xlib-lifetime.c',root/'scripts/test-cairo-xlib-lifetime.py',out/'probe']
for runtime in [prefix]+([a.reference_prefix.resolve()] if a.reference_prefix else []):
 inputs.extend(p for p in (runtime/'lib').glob('*.so*') if p.is_file())
before={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(set(inputs))};(out/'inputs-before.json').write_text(json.dumps(before,indent=2)+'\n')
state={'status':'running','inputs':len(before),'runs':[],'sanitizer_suppressions':False,'scope':'Real Cairo/Xlib source/mask cache; sanitizer client with external optimized bitmap libraries; private Xvfb, no FLTK lifecycle model, physical GPU or session.'};(out/'status.json').write_text(json.dumps(state,indent=2)+'\n')
server=None
try:
 rd,wr=os.pipe();serverlog=(out/'xvfb.log').open('w');server=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','320x240x24','-nolisten','tcp'],pass_fds=(wr,),stdout=serverlog,stderr=serverlog);os.close(wr)
 with os.fdopen(rd) as pipe:env['DISPLAY']=':'+pipe.readline().strip()
 expected_pixels=None
 for label,runtime,expected in ([('old',a.reference_prefix.resolve(),1)] if a.reference_prefix else [])+[('fixed',prefix,0)]:
  t=time.monotonic();runtime_env={**env,'LD_LIBRARY_PATH':str(runtime/'lib')}
  with (out/(label+'.log')).open('w') as f:result=subprocess.run([str(out/'probe')],env=runtime_env,stdout=f,stderr=subprocess.STDOUT,timeout=120)
  text=(out/(label+'.log')).read_text();pixels=re.findall(r'^PIXELS .+$',text,re.M);row={'name':label,'exit_code':result.returncode,'expected_exit':expected,'seconds':time.monotonic()-t,'pixels':pixels,'runtime':str(runtime)};state['runs'].append(row)
  assert result.returncode==expected and len(pixels)==4
  assert 'runtime error:' not in text
  if label=='old':assert 'source' in text and 'LeakSanitizer:' in text;expected_pixels=pixels
  else:
   assert 'LeakSanitizer:' not in text and 'PASS four actual cached-source display lifetimes' in text
   if expected_pixels is not None:assert expected_pixels==pixels
  print(label,result.returncode,pixels,flush=True)
 state['exit_code']=0;state['pixels_exact']=True
except BaseException:
 state['exit_code']=1;state['failure']=traceback.format_exc();raise
finally:
 if server is not None:
  server.terminate();server.wait(timeout=5);serverlog.close()
 after={p:hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in before};state['input_drift']=[p for p in before if before[p]!=after[p]]
 if state['input_drift']:state['exit_code']=1
 (out/'inputs-after.json').write_text(json.dumps(after,indent=2)+'\n');state['status']='terminal';(out/'status.json').write_text(json.dumps(state,indent=2)+'\n')
