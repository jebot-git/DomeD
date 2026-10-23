#!/usr/bin/env python3
"""Private buses/desktop: only the authenticated logind authority can unlock.
Requires dbus-next, python-xlib, Pillow, Xvfb and installed patched FLTK.
"""
import asyncio,json,os,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface,method,signal,dbus_property
from dbus_next.constants import PropertyAccess
from Xlib import X,XK,display
from Xlib.ext import xtest
from PIL import Image
if os.environ.get('DOME_LOCK_TEST_BUS')!='1':
 os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,__file__],{**os.environ,'DOME_LOCK_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];art=root/'artifacts/lightxr-lock';art.mkdir(parents=True,exist_ok=True)
class LoginSession(ServiceInterface):
 def __init__(self):super().__init__('org.freedesktop.login1.Session');self.locked=False
 @dbus_property(access=PropertyAccess.READ)
 def LockedHint(self)->'b':return self.locked
 @signal()
 def Lock(self):return None
 @signal()
 def Unlock(self):return None
class Manager(ServiceInterface):
 def __init__(self):super().__init__('org.freedesktop.login1.Manager')
 @method()
 def GetSession(self,identifier:'s')->'o':
  assert identifier=='fixture';return '/org/freedesktop/login1/session/fixture'
class Seat(ServiceInterface):
 def __init__(self):super().__init__('org.freedesktop.DisplayManager.Seat')
 @dbus_property(access=PropertyAccess.READ)
 def CanSwitch(self)->'b':return True
class DisplaySession(ServiceInterface):
 def __init__(self,login):super().__init__('org.freedesktop.DisplayManager.Session');self.login=login;self.requests=0
 @method()
 def Lock(self):self.requests+=1;self.login.locked=True;self.login.Lock()
async def main():
 bus=await MessageBus().connect();login=LoginSession();login.locked=True;dm=DisplaySession(login)
 bus.export('/org/freedesktop/login1',Manager());bus.export('/org/freedesktop/login1/session/fixture',login)
 bus.export('/org/freedesktop/DisplayManager/Session99',dm);bus.export('/org/freedesktop/DisplayManager/Seat99',Seat())
 await bus.request_name('org.freedesktop.login1');await bus.request_name('org.freedesktop.DisplayManager')
 with tempfile.TemporaryDirectory(prefix='dome-lock-test-') as temp:
  runtime=Path(temp);runtime.chmod(0o700);children=[];logs=[]
  def start(command,env,name,**kw):
   log=(art/name).open('w');logs.append(log);p=subprocess.Popen(list(map(str,command)),env=env,stdout=log,stderr=subprocess.STDOUT,**kw);children.append(p);return p
  http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
  async def wait(fn,timeout=20):
   end=time.monotonic()+timeout
   while time.monotonic()<end:
    try:
     value=fn()
     if value:return value
    except (OSError,ValueError,KeyError,StopIteration):pass
    await asyncio.sleep(.04)
   raise AssertionError('Timeout; inspect artifacts/lightxr-lock')
  async def call(method_name,*args,ok=True):
   p=await asyncio.create_subprocess_exec('gdbus','call','--session','--dest','org.freedesktop.ScreenSaver','--object-path','/ScreenSaver','--method','org.freedesktop.ScreenSaver.'+method_name,*args,stdout=asyncio.subprocess.PIPE,stderr=asyncio.subprocess.PIPE)
   out,err=await p.communicate();assert (p.returncode==0)==ok,(out,err);return out
  try:
   subprocess.run('c++ -O2 -std=c++17 scripts/lock-client.cpp $(/opt/fltk-vr/toolkit/bin/fltk-config --cxxflags --ldflags) -o '+temp+'/client',shell=True,cwd=root,check=True)
   rd,wr=os.pipe();start(['Xvfb','-displayfd',wr,'-screen','0','1280x800x24','-nolisten','tcp','-noreset'],os.environ.copy(),'xvfb.log',pass_fds=(wr,));os.close(wr)
   with os.fdopen(rd) as f:screen=':'+f.readline().strip()
   with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
   env={**os.environ,'DBUS_SYSTEM_BUS_ADDRESS':os.environ['DBUS_SESSION_BUS_ADDRESS'],'DISPLAY':screen,'XDG_RUNTIME_DIR':temp,'XDG_CONFIG_HOME':temp+'/config','XDG_CACHE_HOME':temp+'/cache','XDG_DATA_HOME':temp+'/data','XDG_STATE_HOME':temp+'/state','XDG_SEAT_PATH':'/org/freedesktop/DisplayManager/Seat99','XDG_SESSION_ID':'fixture','XDG_SESSION_PATH':'/org/freedesktop/DisplayManager/Session99','DOME_LOGIN_SESSION':'1','DOME_SESSION_RESTORE':'0','DOME_RENDERER':'gles','DOME_PREVIEW_PORT':str(port),'LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
   for name in ['XR','TRAY','KDECONNECT','POLKIT','KWALLET','IME']:env['DOME_START_'+name]='0'
   for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','FONTCONFIG_FILE']:env.pop(name,None)
   start([os.environ.get('DOME_LOCK_TEST_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor.log')
   def request(path,body=None):
    with http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=2) as r:return json.load(r)
   await wait(lambda:request('/api/state'))
   records=list(runtime.glob('*.dome-session.json'));record=next(p for p in records if json.loads(p.read_text())['realm']=='desktop')
   start([runtime/'client'],{**env,'FLTK_BACKEND':'wayland','WAYLAND_DISPLAY':record.name.removesuffix('.dome-session.json')},'input.log')
   await wait(lambda:any(w['app_id']=='dome-lock-fixture' for w in request('/api/state')['scene']['windows']))
   xd=display.Display(screen);host=await wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
   def key():
    code=xd.keysym_to_keycode(XK.string_to_keysym('a'))
    for kind in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,kind,code)
    xd.sync()
   await wait(lambda:request('/api/state')['scene']['locked'])
   assert not request('/api/xr-layout')['quads'],'Restart exposed an already locked session'
   login.locked=False;login.Unlock()
   await wait(lambda:not request('/api/state')['scene']['locked'])
   key();await wait(lambda:(art/'input.log').read_text().count('KEY')>0)
   before=(art/'input.log').read_text().count('KEY')
   await call('Lock');await wait(lambda:dm.requests==1 and request('/api/state')['scene']['locked'])
   assert b'true' in await call('GetActive')
   await call('SetActive','false',ok=False)
   for _ in range(3):key();await asyncio.sleep(.1)
   assert (art/'input.log').read_text().count('KEY')==before,'Locked keyboard leaked to app'
   layout=request('/api/xr-layout');assert not layout['quads'] and not layout['shell']
   g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
   image=Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX');image.save(art/'shield.png')
   assert image.getbbox() is None,'Locked monitor exposed pixels'
   assert request('/api/state')['scene']['locked'],'Unauthenticated SetActive(false) unlocked'
   login.locked=False;login.Unlock()
   await wait(lambda:not request('/api/state')['scene']['locked'])
   key();await wait(lambda:(art/'input.log').read_text().count('KEY')>before)
   assert request('/api/xr-layout')['quads'],'Desktop did not restore after authority unlock'
   xd.close();print('PASS: already-locked startup, real Wayland client, ScreenSaver Lock, LightXR handoff, blank monitor/XR, blocked input, rejected unauthenticated unlock, trusted logind unlock and restored input')
  finally:
   for child in reversed(children):
    if child.poll() is None:
     child.terminate()
     try:child.wait(timeout=5)
     except subprocess.TimeoutExpired:child.kill();child.wait()
   for log in logs:log.close()
 bus.disconnect()
asyncio.run(main())
