#!/usr/bin/env python3
"""Native GPU-widget and SNI regression on an isolated bus/display."""
import os,sys,time,subprocess,tempfile,json
from pathlib import Path
from Xlib import display,X,XK
from Xlib.ext import xtest
from PIL import Image
ROOT=Path(__file__).resolve().parents[2]
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1'
processes=[]
with tempfile.TemporaryDirectory(prefix='dome-tray-ui-') as directory:
 runtime=Path(directory);runtime.chmod(0o700);out=ROOT/'artifacts/tray-frontends';out.mkdir(exist_ok=True)
 def start(cmd,env,name,**kwargs):
  p=subprocess.Popen(cmd,env=env,stdout=(out/name).open('w'),stderr=subprocess.STDOUT,**kwargs);processes.append(p);return p
 def wait(fn):
  for _ in range(150):
   try:
    value=fn()
    if value:return value
   except Exception:pass
   time.sleep(.04)
  raise AssertionError('Timed out; see artifacts/tray-frontends')
 try:
  rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp'],os.environ.copy(),'ui-xvfb.log',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd) as pipe:disp=':'+pipe.readline().strip()
  env={**os.environ,'DISPLAY':disp,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_CURRENT_DESKTOP':'Dome','DE_XR_GL_TRACE':'1','FLTK_BACKEND':'x11','LIBGL_ALWAYS_SOFTWARE':'1','TRAY_FIXTURE_READY':str(runtime/'ready')}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
  start([sys.executable,str(ROOT/'tests/tray/mock_services.py')],env,'ui-services.log');wait(lambda:(runtime/'ready').exists())
  manager=start([str(ROOT/'build/tray/dome-tray'),'--daemon'],env,'ui-frontend.log')
  def dbus(path,method,*args):
   p=subprocess.run(['gdbus','call','--session','--dest','org.domed.Tray','--object-path',path,'--method',method,*args],env=env,capture_output=True,text=True,timeout=4)
   assert p.returncode==0,p.stderr
   return p.stdout
  wait(lambda:subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.NameHasOwner','org.domed.Tray'],capture_output=True,text=True).stdout.strip()=='(true,)')
  d=display.Display(disp)
  def window():return next((w for w in d.screen().root.query_tree().children if w.get_wm_class() and 'org.domed.Tray' in w.get_wm_class() and w.get_attributes().map_state==X.IsViewable),None)
  def key(name,ctrl=False):
   if ctrl:xtest.fake_input(d,X.KeyPress,d.keysym_to_keycode(XK.string_to_keysym('Control_L')))
   code=d.keysym_to_keycode(XK.string_to_keysym(name));xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code)
   if ctrl:xtest.fake_input(d,X.KeyRelease,d.keysym_to_keycode(XK.string_to_keysym('Control_L')))
   d.sync();time.sleep(.15)
  def click(w,x,y):
   pos=w.translate_coords(d.screen().root,0,0);xtest.fake_input(d,X.MotionNotify,x=pos.x+x,y=pos.y+y)
   xtest.fake_input(d,X.ButtonPress,1);xtest.fake_input(d,X.ButtonRelease,1);d.sync();time.sleep(.3)
  for page,path in [('devices','KDEConnect'),('input','InputMethods'),('wallets','Wallets')]:
   dbus('/'+path,'org.kde.StatusNotifierItem.Activate','0','0');w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.6)
   geo=w.get_geometry();raw=w.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
   img=Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX');assert len(img.getcolors(geo.width*geo.height))>20
   img.save(out/('ui-'+page+'.png'))
   assert 'Open' in dbus('/'+path+'/Menu','com.canonical.dbusmenu.GetLayout','0','1','[]')
   if page=='devices':click(w,80,110)
   if page=='input':click(w,390,424)
   if page=='wallets':click(w,470,95);click(w,80,160)
   key('w',ctrl=True);wait(lambda:window() is None);assert manager.poll() is None
  dbus('/org/domed/Tray','org.domed.Tray.Show','tray');wait(window)
  second=subprocess.run([str(ROOT/'build/tray/dome-tray'),'--devices'],env=env,capture_output=True,timeout=4);assert second.returncode==0
  assert len([w for w in d.screen().root.query_tree().children if w.get_wm_class() and 'org.domed.Tray' in w.get_wm_class()])==1
  log=(out/'ui-frontend.log').read_text();assert 'FLTK GPU widgets:' in log,log
  print('PASS: three native tray icons, exported menus, GPU widget presentation, single-instance routing and Ctrl+W restore')
 finally:
  for p in reversed(processes):p.terminate()
  for p in reversed(processes):
   try:p.wait(timeout=4)
   except subprocess.TimeoutExpired:p.kill();p.wait()
