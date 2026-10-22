#!/usr/bin/env python3
"""Exercise the real FLTK shell selector on a private bus, Wayland server and Xvfb."""
import json,os,re,signal,socket,subprocess,sys,tempfile,time,urllib.request,shutil
from pathlib import Path
from Xlib import display,X,XK
from Xlib.ext import xtest
from PIL import Image
ROOT=Path(__file__).resolve().parents[2]
BUILD=Path(os.environ.get('DOME_TRAY_BUILD_DIR',ROOT/'build/tray'))
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1'
signal.pthread_sigmask(signal.SIG_SETMASK,[])
out=ROOT/'artifacts/keyboard-selector';out.mkdir(parents=True,exist_ok=True)
children=[]
def start(args,env,name,**kwargs):
 p=subprocess.Popen(args,env=env,stdout=(out/(name+'.log')).open('w'),stderr=subprocess.STDOUT,**kwargs);children.append(p);return p
def wait(fn):
 for _ in range(300):
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,StopIteration):pass
  time.sleep(.05)
 raise AssertionError('Timed out; see artifacts/keyboard-selector')
try:
 with tempfile.TemporaryDirectory(prefix='dome-keyboard-') as directory:
  runtime=Path(directory);runtime.chmod(0o700)
  rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],os.environ.copy(),'xvfb',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd) as pipe:disp=':'+pipe.readline().strip()
  with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
  env={**os.environ,'DISPLAY':disp,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_CURRENT_DESKTOP':'Dome','DOME_LOGIN_SESSION':'0','DOME_START_IME':'0','DOME_START_XR':'0','DOME_SESSION_RESTORE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','DOME_PREVIEW_PORT':str(port),'TRAY_FIXTURE_READY':str(runtime/'ready')}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
  env['TRAY_FIXTURE_CONFIG_REQUESTS']=str(runtime/'config-requests')
  applications=runtime/'data/applications';applications.mkdir(parents=True)
  shutil.copy2(ROOT/'packaging/org.domed.Tray.desktop',applications/'org.domed.Tray.desktop')
  icon=runtime/'data/icons/hicolor/scalable/apps/org.domed.Tray.svg';icon.parent.mkdir(parents=True)
  shutil.copy2(ROOT/'native/icons/tray-applications.svg',icon)

  start([sys.executable,str(ROOT/'tests/tray/mock_services.py')],env,'services');wait(lambda:(runtime/'ready').exists())
  start([os.environ.get('DOME_COMPOSITOR',str(ROOT/'target/debug/dome-compositor')),'--backend','nested'],env,'compositor')
  def scene():return json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state',timeout=2))['scene']
  wait(lambda:scene()['source']=='wayland')
  wayland=wait(lambda:re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text()))[1]
  native={**env,'WAYLAND_DISPLAY':wayland,'FLTK_BACKEND':'wayland'}
  manager=start([str(BUILD/'dome-tray'),'--daemon'],native,'frontend')
  def frontend_ready():
   assert manager.poll() is None,(out/'frontend.log').read_text()
   result=subprocess.run(['gdbus','introspect','--session','--dest','org.domed.Tray','--object-path','/InputMethods/Menu'],env=env,capture_output=True,text=True)
   return result.returncode==0 and 'com.canonical.dbusmenu' in result.stdout
  wait(frontend_ready)
  d=display.Display(disp)
  host=wait(lambda:next(w for w in d.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')))
  host.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.6)
  def key(name):
   code=d.keysym_to_keycode(XK.string_to_keysym(name));xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code);d.sync();time.sleep(.15)
  def click(x,y):
   g=host.get_geometry();xtest.fake_input(d,X.MotionNotify,x=g.x+x,y=g.y+y);xtest.fake_input(d,X.ButtonPress,1);xtest.fake_input(d,X.ButtonRelease,1);d.sync();time.sleep(.3)
  def shot(name):
   g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
   image=Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX');image.save(out/(name+'.png'));return image
  def method():
   return subprocess.run(['gdbus','call','--session','--dest','org.fcitx.Fcitx5','--object-path','/controller','--method','org.fcitx.Fcitx.Controller1.CurrentInputMethod'],env=env,capture_output=True,text=True,check=True).stdout
  catalog=json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}/api/apps',timeout=2))
  assert not any(a['desktop_id']=='org.domed.Tray.desktop' for a in catalog), 'Permanent manager must not appear in launcher'
  before=shot('01-us-selector');width=before.width
  assert (255,0,255) not in [color for _,color in before.crop((0,0,width,36)).getcolors(width*36)],'Duplicate Fcitx SNI was rendered'
  assert not scene()['windows'],'Selector must not open a manager window automatically'
  desktop_pixel=before.getpixel((width-270,150))
  click(width-199,18)
  wait(lambda:shot('02-language-menu').getpixel((width-270,150))!=desktop_pixel)
  key('Right')
  def methods_visible():
   image=shot('03-method-submenu')
   return any(color==(190,101,112) for _,color in image.crop((width-254,68,width-230,96)).getcolors(24*28))
  wait(methods_visible)
  key('Down');key('Return')
  wait(lambda:'anthy' in method());time.sleep(.4)
  after=shot('04-japanese-selector')
  assert before.crop((width-220,0,width-176,35)).tobytes()!=after.crop((width-220,0,width-176,35)).tobytes(),'Active flag did not update'
  click(width-199,18)
  wait(lambda:shot('05-config-root').getpixel((width-270,150))!=desktop_pixel)
  key('End');key('Home');key('Down');key('Down');key('Right')
  def configuration_visible():
   image=shot('05-config-submenu')
   return image.getpixel((width-270,150))!=desktop_pixel and image.getpixel((width-270,170))==desktop_pixel
  wait(configuration_visible)
  key('Down');key('Return')
  wait(lambda:any(w['app_id']=='org.domed.Tray' for w in scene()['windows']));time.sleep(.5)
  wait(lambda:(runtime/'config-requests').exists() and 'fcitx://config/global' in (runtime/'config-requests').read_text())
  time.sleep(.3)
  shot('06-fltk-configuration')
  assert manager.poll() is None
  # Invoke the manager's real File > Close through the compositor's universal menu.
  # The hidden local menu must remain exported and closing must keep the tray alive.
  click(255,18);time.sleep(.4);shot('07-universal-file')
  key('Home');key('Down');key('Return')
  wait(lambda:not any(w['app_id']=='org.domed.Tray' for w in scene()['windows']))
  assert manager.poll() is None
  print('PASS: native Wayland universal File > Close hides the manager and preserves its tray service')
  driver=subprocess.Popen([str(BUILD/'tray-service-driver')],env=native,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True);children.append(driver)
  def service(op,**args):
   driver.stdin.write(json.dumps({'op':op,**args})+'\n');driver.stdin.flush()
   value=json.loads(driver.stdout.readline());assert value['ok'],value;return value['value']
  fixture=next(i for i in service('tray') if i['Id']=='fixture')
  screenshot=next(i for i in service('tray') if i['Id']=='dome.screenshot')
  def green():
   image=shot('08-tray-placement')
   return [(x,y) for y in range(6,30) for x in range(width) if image.getpixel((x,y))==(0,255,0)]
  assert green(),'Fixture icon must begin in its individual tray placement'
  service('placement',item=fixture,managed=True);wait(lambda:not green())
  assert next(i for i in service('tray') if i['Id']=='fixture')['managed']
  service('placement',item=fixture,managed=False);wait(green)
  service('placement',item=screenshot,managed=True);time.sleep(.4)
  point=green()[0];click(point[0]+4,18);key('Return')
  wait(lambda:Path(str(runtime/'ready')+'.event').exists())
  service('placement',item=screenshot,managed=False)
  print('PASS: move applications into/out of manager live, persist placement, and route tray actions after screenshot moves')
  print('PASS: flag updates, Fcitx SNI suppression, keyboard submenu navigation and native Wayland configuration frontend')
finally:
 for p in reversed(children):p.terminate()
 for p in reversed(children):
  try:p.wait(timeout=4)
  except subprocess.TimeoutExpired:p.kill();p.wait()
