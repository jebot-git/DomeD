import os,subprocess,tempfile,time,socket,json,urllib.request
from pathlib import Path
from Xlib import display,X,XK
from Xlib.ext import xtest
root=Path(__file__).resolve().parents[2]; procs=[]
with tempfile.TemporaryDirectory(prefix='dome-askpass-wayland-') as directory:
 r=Path(directory);r.chmod(0o700)
 out=root/'artifacts/askpass';out.mkdir(exist_ok=True)
 def start(cmd,env,log,**kw):
  p=subprocess.Popen(cmd,env=env,stdout=(out/log).open('w'),stderr=subprocess.STDOUT,**kw);procs.append(p);return p
 try:
  rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1024x768x24','-nolisten','tcp'],os.environ.copy(),'wayland-xvfb.log',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd) as f:disp=':'+f.readline().strip()
  with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  probe=r/'probe.py';marker=r/'result.json'
  probe.write_text('import os,subprocess,json\nfrom pathlib import Path\np=subprocess.run([os.environ["SUDO_ASKPASS"],"Disposable Wayland test"],capture_output=True,env={**os.environ,"WAYLAND_DISPLAY":"wayland-missing","DISPLAY":":99999","XDG_CURRENT_DESKTOP":"KDE"})\nPath('+repr(str(marker))+').write_text(json.dumps({"helper":os.environ["SUDO_ASKPASS"],"code":p.returncode,"correct":p.stdout==b"ab5\\n"}))\n')
  sibling=root/'target/release/dome-askpass';assert not sibling.exists(), 'Test helper path already exists';sibling.write_text('#!/bin/sh\nexec "'+str(root/'build/askpass-stage/usr/local/bin/dome-askpass')+'" "$@"\n');sibling.chmod(0o755)
  env={**os.environ,'DISPLAY':disp,'XDG_RUNTIME_DIR':directory,'XDG_CURRENT_DESKTOP':'Dome','DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_START_IME':'0','DOME_RENDERER':'gles','DOME_PREVIEW_PORT':str(port),'LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn','XDG_CONFIG_HOME':str(r/'config'),'XDG_STATE_HOME':str(r/'state'),'XDG_DATA_HOME':str(r/'data'),'FLTK_BACKEND':'wayland'}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','SUDO_ASKPASS']:env.pop(key,None)
  c=start(['dbus-run-session','--',str(root/'target/release/dome-compositor'),'--backend','nested','--command','/usr/bin/python3',str(probe)],env,'wayland-compositor.log')
  for _ in range(200):
   try:
    state=json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state',timeout=1))
    if 'org.domed.Askpass' in json.dumps(state):break
   except Exception:pass
   if c.poll() is not None:raise AssertionError('compositor exited')
   time.sleep(.1)
  else:raise AssertionError('No native Wayland dialog')
  d=display.Display(disp)
  host=next(w for w in d.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome'))
  host.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.3)
  for name in ['a','b','5','Return']:
   code=d.keysym_to_keycode(XK.string_to_keysym(name))
   xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code);d.sync();time.sleep(.08)
  for _ in range(100):
   if marker.exists():break
   time.sleep(.05)
  result=json.loads(marker.read_text());assert result['correct'] and result['code']==0,result
  (out/'wayland-result.json').write_text(json.dumps(result,indent=2))
  print('PASS native Wayland entry, compositor inheritance and recovery from stale display/desktop environment')
 finally:
  for p in reversed(procs):p.terminate()
  for p in reversed(procs):
   try:p.wait(timeout=5)
   except subprocess.TimeoutExpired:p.kill()
  if os.path.ismount(r/'doc'):
   subprocess.run(['fusermount3','-u',str(r/'doc')],check=True)
  if 'sibling' in locals() and sibling.exists() and 'build/askpass-stage' in sibling.read_text():sibling.unlink()
