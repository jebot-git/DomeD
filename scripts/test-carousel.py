#!/usr/bin/env python3
"""Private Xvfb keyboard regression; requires python-xlib and PySide6 fixtures."""
import os,sys,json,tempfile,subprocess,socket,time,urllib.request,re,threading
from pathlib import Path
from Xlib import X,XK,display
from Xlib.ext import xtest
if os.environ.get('PROBE_BUS')!='1':os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'PROBE_BUS':'1'})
root=Path(__file__).resolve().parents[1];children=[]
op=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def wait(fn):
 end=time.monotonic()+20
 while time.monotonic()<end:
  try:
   v=fn()
   if v:return v
  except (OSError,ValueError):pass
  time.sleep(.03)
 raise AssertionError('timeout')
try:
 with tempfile.TemporaryDirectory(prefix='carousel-probe-') as d:
  r=Path(d);r.chmod(0o700)
  env={**os.environ,'HOME':d,'XDG_RUNTIME_DIR':d,'XDG_CONFIG_HOME':d+'/config','XDG_CACHE_HOME':d+'/cache','XDG_DATA_HOME':d+'/data','XDG_STATE_HOME':d+'/state','DOME_LOGIN_SESSION':'0','DOME_SESSION_RESTORE':'0','DOME_START_XR':'0','DOME_START_IME':'0','DOME_START_TRAY':'0','DOME_START_POLKIT':'0','DOME_START_KWALLET':'0','DOME_START_KDECONNECT':'0','LIBGL_ALWAYS_SOFTWARE':'1','DOME_RENDERER':'gles','RUST_LOG':'warn'}
  for k in ['DISPLAY','WAYLAND_DISPLAY','WAYLAND_SOCKET','FONTCONFIG_FILE']:env.pop(k,None)
  rd,wr=os.pipe();children.append(subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp','-noreset'],pass_fds=(wr,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,env=env));os.close(wr)
  with os.fdopen(rd) as f:env['DISPLAY']=':'+f.readline().strip()
  with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  env['DOME_PREVIEW_PORT']=str(port);base=f'http://127.0.0.1:{port}'
  def req(route,data=None):
   with op.open(urllib.request.Request(base+route,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json'}),timeout=2) as f:
    raw=f.read();return json.loads(raw) if raw and raw[:1] in [b'{',b'['] else raw
  def state():return req('/api/state')['scene']
  log=(r/'compositor.log').open('w');children.append(subprocess.Popen([os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env=env,stdout=log,stderr=subprocess.STDOUT));wait(state)
  wayland=re.search(r'Dome Wayland socket: (\S+)',(r/'compositor.log').read_text())[1]
  ce={**env,'WAYLAND_DISPLAY':wayland,'QT_QPA_PLATFORM':'wayland'}
  for name in ['a','b','c']:
   children.append(subprocess.Popen([sys.executable,str(root/'scripts/desktop-client.py'),'--name',name,'--journal',str(r/(name+'.jsonl'))],env=ce,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL))
  wait(lambda:len(state()['windows'])==3)
  ids=state()['workspaces'][0]['lanes'][1]['windows'];req('/api/action',{'op':'focus','id':ids[1]})
  xd=display.Display(env['DISPLAY']);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.3)
  def key(name,down):xtest.fake_input(xd,X.KeyPress if down else X.KeyRelease,xd.keysym_to_keycode(XK.string_to_keysym(name)));xd.sync();time.sleep(.04)
  def chord(arrow):
   for k in ['Super_L','Alt_L']:key(k,True)
   key(arrow,True);key(arrow,False)
   for k in ['Alt_L','Super_L']:key(k,False)
   time.sleep(.2)
  lane=lambda:state()['workspaces'][0]['lanes'][1]['windows']
  print('before',lane(),flush=True);chord('Left');print('after left',lane(),flush=True);assert lane().index(ids[1])==0
  chord('Right');print('after right',lane(),flush=True);assert lane()==ids
  print('PASS native desktop keyboard reorders both ways',flush=True)
  stop=threading.Event()
  def heartbeat():
   while not stop.is_set():
    req('/api/xr-session',{'visible':True,'focused':True,'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':100})
    stop.wait(.1)
  pulse=threading.Thread(target=heartbeat,daemon=True);pulse.start()
  wait(lambda:req('/api/xr-control')['input_realm']=='xr')
  req('/api/action',{'op':'focus','id':ids[1]});time.sleep(.3)
  chord('Left');print('XR after left',lane(),flush=True);assert lane().index(ids[1])==0
  chord('Right');print('XR after right',lane(),flush=True);assert lane()==ids
  req('/api/action',{'op':'window_visibility','id':ids[0],'desktop':True,'xr':False})
  req('/api/action',{'op':'focus','id':ids[1]});time.sleep(.2)
  def yaw():return next(p['yaw_deg'] for p in req('/api/state')['panels'] if p['id']==ids[1])
  before_yaw=yaw();chord('Left')
  assert lane()==[ids[0],ids[2],ids[1]],('hidden window consumed Left',lane())
  assert abs(yaw()-before_yaw)>1,'Visible panel must actually change angular position'
  chord('Right');assert lane()==ids
  assert abs(yaw()-before_yaw)<.001
  print('PASS native arrows skip hidden windows, change panel geometry, and wrap both edges',flush=True)
  stop.set();pulse.join(timeout=2)
  print('PASS native XR keyboard reorders both ways',flush=True)
  xd.close()
finally:
 for c in reversed(children):
  if c.poll() is None:c.terminate()
  try:c.wait(timeout=4)
  except subprocess.TimeoutExpired:c.kill();c.wait()
