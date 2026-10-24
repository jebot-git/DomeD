#!/usr/bin/env python3
"""Actual launcher gestures, persistence, keyboard focus and XR pointer routing."""
import json,os,signal,socket,subprocess,sys,tempfile,threading,time,urllib.request
from pathlib import Path
from PIL import Image,ImageChops
from Xlib import X,XK,display
from Xlib.ext import xtest
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_QUICK_TEST_BUS')!='1':os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_QUICK_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/launcher-quick-launch';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];checks=[];stop=threading.Event()
def wait(fn,seconds=12):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   result=fn()
   if result:return result
  except(OSError,ValueError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('Timed out; inspect '+str(out))
def check(ok,name):assert ok,name;checks.append(name);print('PASS '+name,flush=True)
def start(args,env,name,**kwargs):
 f=(out/(name+'.log')).open('w');logs.append(f);p=subprocess.Popen(args,env=env,stdout=f,stderr=f,**kwargs);children.append(p);return p
try:
 with tempfile.TemporaryDirectory(prefix='dome-quick-launch-')as tmp:
  runtime=Path(tmp);runtime.chmod(0o700);apps=runtime/'data/applications';apps.mkdir(parents=True)
  helper=runtime/'launch.py';journal=runtime/'launches'
  helper.write_text('import sys\nfrom pathlib import Path\nwith Path('+repr(str(journal))+').open("a") as f:f.write(sys.argv[1]+"\\n")\n')
  for i in range(15):(apps/f'pinfixture{i:02}.desktop').write_text(f'[Desktop Entry]\nType=Application\nName=PinFixture {i:02}\nExec=/usr/bin/python3 {helper} {i:02}\nIcon=application-x-executable\nCategories=Utility;\n')
  rd,wr=os.pipe();xvfb=start(['Xvfb','-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],os.environ.copy(),'xvfb',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd)as f:screen=':'+f.readline().strip()
  with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  env={**os.environ,'DISPLAY':screen,'XDG_RUNTIME_DIR':tmp,'XDG_CONFIG_HOME':tmp+'/config','XDG_STATE_HOME':tmp+'/state','XDG_CACHE_HOME':tmp+'/cache','XDG_DATA_HOME':tmp+'/data','XDG_CURRENT_DESKTOP':'Dome','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_APPEARANCE_SERVICE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','FLTK_BACKEND':'wayland'}
  for name in('WAYLAND_DISPLAY','WAYLAND_SOCKET','DOME_CONFIG'):env.pop(name,None)
  for service in('TRAY','KDECONNECT','KWALLET','POLKIT','IME'):env['DOME_START_'+service]='0'
  compositor=start([os.environ.get('DOME_QUICK_TEST_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor')
  http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
  def request(path,body=None):return http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=3)
  def read(path):
   with request(path)as response:return json.load(response)
  shell=lambda:read('/api/xr-shell');wait(lambda:shell()['width'])
  xd=display.Display(screen);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
  def key(name,down=None):
   code=xd.keysym_to_keycode(XK.string_to_keysym(name))
   for kind in((X.KeyPress,X.KeyRelease)if down is None else(X.KeyPress if down else X.KeyRelease,)):xtest.fake_input(xd,kind,code);xd.sync();time.sleep(.04)
  def motion(x,y):
   g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.05)
  def button(down,number=1):xtest.fake_input(xd,X.ButtonPress if down else X.ButtonRelease,number);xd.sync();time.sleep(.06)
  def click(x,y,shift=False,number=1):
   if shift:key('Shift_L',True)
   motion(x,y);button(True,number);button(False,number)
   if shift:key('Shift_L',False)
   time.sleep(.1)
  def text(value):
   for c in value:key('space'if c==' 'else c)
  def capture(name):
   g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);im=Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX');im.save(out/(name+'.png'));return im
  file=runtime/'config/dome/launcher.json'
  pins=lambda:json.loads(file.read_text())['desktop_ids']
  def open_launcher(query=True):
   key('Super_L',True);key('Shift_L',True);key('space');key('Shift_L',False);key('Super_L',False);wait(lambda:shell()['top']==456)
   if query:text('pinfixture')
  def drag(x1,x2,y=420,shift=True,outside=False):
   if shift:key('Shift_L',True)
   motion(x1,y);button(True)
   for i in range(1,6):motion(x1+(x2-x1)*i/5,y)
   if outside:motion(800,500)
   button(False)
   if shift:key('Shift_L',False)
  open_launcher();empty=capture('00-empty');click(150,120,True);wait(lambda:file.exists());one=capture('00-one-pin')
  diff=ImageChops.difference(empty,one).crop((28,396,88,438));check(sum(any(c for c in pixel)for pixel in diff.getdata())>500,'Pinning immediately redraws the quick-launch icon and selection')
  check(pins()==['pinfixture00.desktop']and not journal.exists()and shell()['top']==456,'Shift-click pins without launching or closing the launcher')
  click(150,156,True);click(150,192,True);click(150,120,True)
  check(pins()==['pinfixture00.desktop','pinfixture01.desktop','pinfixture02.desktop'],'Pins keep stable desktop IDs, append left-aligned slots and avoid duplicates')
  drag(36+2*48+20,36+20);wait(lambda:pins()[0]=='pinfixture02.desktop')
  capture('01-reordered-desktop')
  check(pins()==['pinfixture02.desktop','pinfixture00.desktop','pinfixture01.desktop'],'Shift-drag reorders snapped quick-launch slots and persists on drop')
  before=pins();drag(56,152,outside=True);check(pins()==before,'Dropping outside quick launch cancels reordering')
  click(36+48+20,420,True,3);wait(lambda:len(pins())==2)
  check(pins()==['pinfixture02.desktop','pinfixture01.desktop'],'Shift-RMB removes only the targeted pin')
  key('Escape');open_launcher();check(pins()==['pinfixture02.desktop','pinfixture01.desktop'],'Pinned order survives closing and reopening the launcher')
  key('Tab');key('Tab');key('Return');wait(lambda:journal.exists()and'01'in journal.read_text())
  check(journal.read_text().splitlines()==['01'],'Tab cycles quick-launch focus and Enter launches the selected pin instead of search results')
  open_launcher()
  key('Tab');key('Shift_L',True);key('Tab');key('Shift_L',False);key('Return');wait(lambda:len(journal.read_text().splitlines())==2)
  check(journal.read_text().splitlines()==['01','01'],'Shift-Tab cycles quick-launch entries backwards')
  # Seed overflow through the same validated on-disk format, then verify actual
  # keyboard paging and launch dispatch; this leaves the real user's file alone.
  file.write_text(json.dumps({'desktop_ids':[f'pinfixture{i:02}.desktop'for i in range(15)]}))
  open_launcher()
  for i in range(15):key('Tab')
  overflow=capture('02-keyboard-overflow');check(overflow.getpixel((513,399))==one.getpixel((33,399)),'Tab updates the visible overflow page and quick-launch highlight immediately');key('Return');wait(lambda:len(journal.read_text().splitlines())==3)
  check(journal.read_text().splitlines()[-1]=='14','Tab reveals and launches every pin beyond the visible quick-launch row')
  open_launcher(False);text('unmatched');key('Tab');key('Return');wait(lambda:len(journal.read_text().splitlines())==4)
  check(journal.read_text().splitlines()[-1]=='00','Quick-launch activation uses its stable application ID independently of the search query')
  # Exercise the same gestures through the XR shell packet route.
  def beat():
   while not stop.wait(.1):
    try:request('/api/xr-session',{'id':433,'visible':True,'focused':True,'head':{'position_m':[0,1.6,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':90}).close()
    except OSError:pass
  threading.Thread(target=beat,daemon=True).start();wait(lambda:read('/api/xr-control')['input_realm']=='xr')
  def xr_motion(x,y,bar=0):
   l=shell()
   if l.get('pullout') and bar==0 and y>=36:bar=2**64-6;uv=[x/l['width'],(y-36)/(min(l['top'],l['height']-60)-36)]
   elif l.get('pullout'):uv=[x/l['width'],y/36] if bar==0 else [x/l['width'],(y-l['height']+60)/60]
   else:uv=[x/l['width'],y/l['top']]if bar==0 else[x/l['width'],(y-l['height']+l['bottom'])/l['bottom']]
   request('/api/xr-input',{'actions':[],'events':[{'kind':'motion','id':bar,'uv':uv}]}).close();time.sleep(.1)
  def xr_button(pressed,button=272):request('/api/xr-input',{'actions':[],'events':[{'kind':'button','button':button,'pressed':pressed}]}).close();time.sleep(.1)
  xr_motion(50,shell()['height']-30,2**64-3);xr_button(True);xr_button(False);wait(lambda:shell()['top']==456)
  text('pinfixture');key('Shift_L',True);xr_motion(56,420);xr_button(True)
  xr_motion(152,420);xr_button(False);key('Shift_L',False);wait(lambda:pins()[2]=='pinfixture00.desktop')
  key('Shift_L',True);xr_motion(56,420);xr_button(True,273);xr_button(False,273);key('Shift_L',False);wait(lambda:len(pins())==14)
  check(pins()[:3]==['pinfixture02.desktop','pinfixture00.desktop','pinfixture03.desktop'],'XR Shift-drag and Shift-RMB share desktop snapping, persistence and removal semantics')
  capture('03-xr-quick-launch')
  with request('/api/xr-frame/'+str(2**64-6))as response:
   Image.frombytes('RGBA',(int(response.headers['X-Dome-Width']),int(response.headers['X-Dome-Height'])),response.read()).save(out/'04-xr-texture.png')
  xd.close()
  (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
finally:
 if sys.exc_info()[0] and 'capture' in locals():
  try:capture('failure');(out/'failure-state.json').write_text(json.dumps(read('/api/state'),indent=2));(out/'failure-shell.json').write_text(json.dumps(shell(),indent=2))
  except Exception:pass
 stop.set()
 for child in reversed(children):
  if child.poll()is None:
   child.terminate()
   try:child.wait(timeout=4)
   except subprocess.TimeoutExpired:child.kill();child.wait()
 for f in logs:f.close()
