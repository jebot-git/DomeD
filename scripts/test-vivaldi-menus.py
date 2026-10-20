#!/usr/bin/env python3
"""Real Vivaldi floating-menu navigation on an isolated display, bus and browser profile.
Run with uv --with python-xlib --with pillow. Set DOME_XVFB and DOME_VIVALDI as needed.
"""
import json,os,re,signal,socket,subprocess,sys,tempfile,time,urllib.request,shutil,threading
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_VIVALDI_TEST_BUS')!='1':
 os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_VIVALDI_TEST_BUS':'1'})
from Xlib import X,XK,display
from Xlib.ext import xtest
from PIL import Image
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/vivaldi-menus';out.mkdir(parents=True,exist_ok=True)
procs=[];logs=[];heartbeat_stop=threading.Event();pulse=None
def start(args,name,env,**kw):
 f=(out/(name+'.log')).open('w');logs.append(f);p=subprocess.Popen(args,env=env,stdout=f,stderr=f,start_new_session=True,**kw);procs.append(p);return p
def wait(fn):
 end=time.monotonic()+60
 while time.monotonic()<end:
  try:
   v=fn()
   if v:return v
  except Exception:pass
  time.sleep(.1)
 raise RuntimeError('timeout')
try:
 tmp=tempfile.mkdtemp(prefix='dome-vivaldi-')
 os.chmod(tmp,0o700);rd,wr=os.pipe()
 start([os.environ.get('DOME_XVFB','Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
 with os.fdopen(rd) as f:ds=':'+f.readline().strip()
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 env={**os.environ,'HOME':tmp,'XDG_CONFIG_HOME':tmp+'/config','XDG_DATA_HOME':tmp+'/data','XDG_STATE_HOME':tmp+'/state','XDG_RUNTIME_DIR':tmp,'DISPLAY':ds,'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_START_IME':'0','DOME_RENDERER':'gles','DOME_PREVIEW_PORT':str(port),'LIBGL_ALWAYS_SOFTWARE':'1'}
 for k in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM']:env.pop(k,None)
 subprocess.run(['dbus-update-activation-environment','HOME','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','DISPLAY','QT_QPA_PLATFORM=offscreen'],env=env,check=True)
 start([os.environ.get('DOME_COMPOSITOR',str(root/'target/debug/dome-compositor')),'--backend','nested'],'compositor',env)
 base=f'http://127.0.0.1:{port}'
 def scene():return json.load(urllib.request.urlopen(base+'/api/state'))['scene']
 wait(lambda:scene()['source']=='wayland')
 wl=re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text())[1]
 profile=Path(tmp)/'profile/Default';profile.mkdir(parents=True)
 (profile/'Preferences').write_text(json.dumps({'vivaldi':{'startup':{'has_seen_welcome_page':True},'welcome':{'version':'8.2'}}}))
 # The disposable profile must not wait for a new KWallet's password dialog.
 start([os.environ.get('DOME_VIVALDI',shutil.which('vivaldi')or'vivaldi'),'--user-data-dir='+tmp+'/profile','--ozone-platform=wayland','--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-features=Vulkan','--disable-gpu','--password-store=basic','about:blank'],'vivaldi',{**env,'WAYLAND_DISPLAY':wl,'GDK_BACKEND':'wayland','QT_QPA_PLATFORM':'wayland'})
 wait(lambda:len(scene()['windows'])>0);time.sleep(8)
 xd=display.Display(ds)
 host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
 host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
 def shot(name):
  g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/(name+'.png'));(out/(name+'.json')).write_text(json.dumps(scene(),indent=2))
 def key(n,down):xtest.fake_input(xd,X.KeyPress if down else X.KeyRelease,xd.keysym_to_keycode(XK.string_to_keysym(n)));xd.sync();time.sleep(.1)
 time.sleep(2)
 shot('initial')
 key('Alt_L',True);key('f',True);key('f',False);key('Alt_L',False);time.sleep(2);shot('menu')
 popups=scene()['popups'];root_popup=next(p for p in popups if p['parent']==1);child=next(p for p in popups if p['parent']==root_popup['id'])
 assert root_popup['anchor']['x']>0
 def move(x,y):
  g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.2)
 rx=root_popup['anchor']['x'];ry=36+root_popup['anchor']['y'];cx=rx+child['anchor']['x'];cy=ry+child['anchor']['y']
 move(rx+100,ry+20);move(cx+4,cy+20);move(cx+100,cy+58)
 shot('submenu-hover')
 assert len(scene()['popups'])==2,scene()['popups']
 for typ in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(xd,typ,1);xd.sync();time.sleep(.1)
 wait(lambda:len(scene()['windows'])==2)
 print('PASS Vivaldi: compositor-configured floating menus, pointer traversal into submenu, New Window action',flush=True)
 def post(path,value):
  urllib.request.urlopen(urllib.request.Request(base+path,data=json.dumps(value).encode(),headers={'Content-Type':'application/json'}),timeout=2).close()
 def press(name):key(name,True);key(name,False)
 def click(x,y):
  move(x,y)
  for typ in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(xd,typ,1);xd.sync();time.sleep(.1)
 wid=scene()['focused']
 click(430,18);press('Home');press('Return')
 wait(lambda:not next(w for w in scene()['windows'] if w['id']==wid)['xr_visible'])
 print('PASS Vivaldi: universal Window sends the focused binary window to Desktop',flush=True)
 def heartbeat():
  while not heartbeat_stop.is_set():
   post('/api/xr-session',{'visible':True,'focused':False,'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':100})
   heartbeat_stop.wait(.1)
 pulse=threading.Thread(target=heartbeat,daemon=True);pulse.start();time.sleep(.5)
 # A visible simulated XR session owns physical input. Route shell clicks just
 # like the real XR frontend, and start with an XR-only focused browser window.
 post('/api/action',{'op':'window_visibility','id':wid,'desktop':False,'xr':True})
 post('/api/action',{'op':'focus','id':wid});time.sleep(.5)
 def xr_shell_click(x,y):
  layout=json.load(urllib.request.urlopen(base+'/api/xr-shell',timeout=2))
  post('/api/xr-input',{'events':[{'kind':'motion','id':0,'uv':[x/layout['width'],y/layout['top']]}],'actions':[]});time.sleep(.2)
  post('/api/xr-input',{'events':[{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}],'actions':[]});time.sleep(.3)
 xr_shell_click(430,18);press('Home');press('Down');press('Down');press('Return')
 wait(lambda:all(next(w for w in scene()['windows'] if w['id']==wid)[k] for k in ['desktop_visible','xr_visible']))
 xr_shell_click(430,18);shot('universal-window-both');press('Escape')
 print('PASS Vivaldi: universal Window restores Both with simulated XR availability',flush=True)
 xr_shell_click(430,18);press('Home');press('Down');press('Return')
 wait(lambda:not next(w for w in scene()['windows'] if w['id']==wid)['desktop_visible'])
 print('PASS Vivaldi: universal Window sends the focused binary window to XR',flush=True)
 post('/api/action',{'op':'window_visibility','id':wid,'desktop':True,'xr':True})
 heartbeat_stop.set();pulse.join(timeout=3)
 post('/api/xr-session',{'visible':False,'focused':False})
 post('/api/action',{'op':'focus','id':wid});time.sleep(.5)
 click(260,18);press('Home');press('Return')
 wait(lambda:not any(w['id']==wid for w in scene()['windows']))
 print('PASS Vivaldi: universal File closes its focused window through native client protocol',flush=True)
 print(out,flush=True)
finally:
 heartbeat_stop.set()
 if pulse:pulse.join(timeout=3)
 for p in reversed(procs):
  try:os.killpg(p.pid,signal.SIGTERM)
  except ProcessLookupError:pass
 for p in reversed(procs):
  try:p.wait(timeout=5)
  except subprocess.TimeoutExpired:
   try:os.killpg(p.pid,signal.SIGKILL)
   except ProcessLookupError:pass
   p.wait()

 if "tmp" in globals():
  subprocess.run(["fusermount3","-u",str(Path(tmp)/"doc")],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
  shutil.rmtree(tmp,ignore_errors=True)
