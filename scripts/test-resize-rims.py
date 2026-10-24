#!/usr/bin/env python3
"""Real native FLTK diagonal rim drags on a disposable DomeD seat."""
import json,os,signal,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from PIL import Image
from Xlib import X,display
from Xlib.ext import xtest
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_RIMS_TEST_BUS')!='1':os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_RIMS_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/resize-rims';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];checks=[]
def wait(fn,seconds=15):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   value=fn()
   if value:return value
  except(OSError,ValueError,KeyError):pass
  time.sleep(.05)
 raise AssertionError('Timed out; inspect '+str(out))
def check(ok,name):assert ok,name;checks.append(name);print('PASS '+name,flush=True)
def start(args,env,name,**kwargs):
 f=(out/(name+'.log')).open('w');logs.append(f);p=subprocess.Popen(args,env=env,stdout=f,stderr=f,**kwargs);children.append(p);return p
try:
 with tempfile.TemporaryDirectory(prefix='dome-resize-rims-')as tmp:
  runtime=Path(tmp);runtime.chmod(0o700);source=runtime/'client.cpp';marker=runtime/'events'
  source.write_text('''#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <fstream>
static const char* journal;
class Window: public Fl_Window {public:using Fl_Window::Fl_Window;
int handle(int event)override {if(event==FL_PUSH){std::ofstream(journal,std::ios::app)<<"press\\n";return 1;}return Fl_Window::handle(event);}
void resize(int X,int Y,int W,int H)override{Fl_Window::resize(X,Y,W,H);std::ofstream(journal,std::ios::app)<<"size "<<W<<" "<<H<<"\\n";}};
int main(int argc,char**argv){journal=argv[1];Window w(400,260,"Resize rims fixture");w.xclass("org.domed.ResizeRimsFixture");w.color(fl_rgb_color(56,78,92));w.end();w.resizable(&w);w.size_range(160,100);w.show();return Fl::run();}
''')
  prefix=Path(os.environ.get('DOME_FLTK_PREFIX','/opt/fltk-vr/toolkit'));flags=subprocess.check_output(['pkg-config','--cflags','--libs','cairo'],text=True).split()
  subprocess.run(['g++','-std=c++17','-O2','-I'+str(prefix/'include'),str(source),'-L'+str(prefix/'lib'),'-Wl,-rpath,'+str(prefix/'lib'),'-lfltk',*flags,'-o',str(runtime/'client')],check=True)
  rd,wr=os.pipe();xvfb=start(['Xvfb','-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],os.environ.copy(),'xvfb',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd)as f:screen=':'+f.readline().strip()
  with socket.socket()as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
  env={**os.environ,'DISPLAY':screen,'XDG_RUNTIME_DIR':tmp,'XDG_CONFIG_HOME':tmp+'/config','XDG_STATE_HOME':tmp+'/state','XDG_CACHE_HOME':tmp+'/cache','XDG_DATA_HOME':tmp+'/data','XDG_CURRENT_DESKTOP':'Dome','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_APPEARANCE_SERVICE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','FLTK_BACKEND':'wayland','LD_LIBRARY_PATH':str(prefix/'lib')}
  for key in('WAYLAND_DISPLAY','WAYLAND_SOCKET','DOME_CONFIG'):env.pop(key,None)
  for service in('TRAY','KDECONNECT','POLKIT','KWALLET','IME'):env['DOME_START_'+service]='0'
  compositor=start([os.environ.get('DOME_RIMS_TEST_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor')
  http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
  def read(path):
   with http.open(f'http://127.0.0.1:{port}'+path,timeout=3)as response:return json.load(response)
  state=lambda:read('/api/state')['scene'];wait(state)
  desktop=wait(lambda:next((p for p in runtime.glob('*.dome-session.json')if json.loads(p.read_text())['realm']=='desktop'),None))
  client=start([str(runtime/'client'),str(marker)],{**env,'WAYLAND_DISPLAY':desktop.name.removesuffix('.dome-session.json')},'client')
  wid=wait(lambda:next((w['id']for w in state()['windows']if w['app_id']=='org.domed.ResizeRimsFixture'),None))
  def rect():return next(w['desktop']for w in state()['windows']if w['id']==wid)
  xd=display.Display(screen);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
  def motion(x,y):
   g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.05)
  def button(down):xtest.fake_input(xd,X.ButtonPress if down else X.ButtonRelease,1);xd.sync();time.sleep(.08)
  def drag(x,y,dx,dy):
   motion(x,y);button(True)
   for step in range(1,7):motion(x+dx*step/6,y+dy*step/6)
   button(False)
  def close(a,b):return abs(a-b)<3
  for left,side,halo in[(True,False,False),(False,False,False),(True,True,False),(False,True,False),(True,True,True),(False,False,True)]:
   old=rect();dx=-18 if left else 18;dy=12
   x=old['x']+((-6 if halo else -2) if left else old['w']+(6 if halo else 2))if side else old['x']+(12 if left else old['w']-12)
   y=old['y']+old['h']+(-12 if side else (6 if halo else 2))
   drag(x,y,dx,dy)
   wait(lambda:close(rect()['w'],old['w']+18)and close(rect()['h'],old['h']+12))
   now=rect();check(close(now['x']+(now['w']if left else 0),old['x']+(old['w']if left else 0))and close(now['y'],old['y']),('Left'if left else'Right')+(' outer margin'if halo else(' side-leg'if side else' bottom-leg'))+' grip resizes diagonally and anchors the opposite edges')
   wait(lambda:f"size {int(now['w'])} {int(now['h'])}"in marker.read_text())
  before=rect();presses=marker.read_text().splitlines().count('press')
  for x in(before['x']+12,before['x']+before['w']-12):motion(x,before['y']+before['h']-12);button(True);button(False)
  wait(lambda:marker.read_text().splitlines().count('press')==presses+2)
  check(rect()==before,'Nearby client content remains clickable and never starts a rim resize')
  g=host.get_geometry();pixels=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),pixels.data,'raw','BGRX').save(out/'fltk-resize-rims.png')
  check(client.poll()is None,'Native FLTK client acknowledges every resize and remains responsive')
  xd.close();(out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
finally:
 for child in reversed(children):
  if child.poll()is None:
   child.terminate()
   try:child.wait(timeout=4)
   except subprocess.TimeoutExpired:child.kill();child.wait()
 for f in logs:f.close()
