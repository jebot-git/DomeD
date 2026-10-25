#!/usr/bin/env python3
"""Real Wayland launch sizes, unchanged FLTK scale, half-resolution dialogs, fixed clients and VRCX."""
import json,math,os,signal,socket,subprocess,tempfile,threading,time,urllib.request
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_SIZE_TEST_BUS')!='1':os.execvpe('dbus-run-session',['dbus-run-session','--',os.sys.executable,*os.sys.argv],{**os.environ,'DOME_SIZE_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/xr-window-resolution';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];stop=threading.Event();results={}
def wait(fn,seconds=15):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:
   value=fn()
   if value:return value
  except (OSError,ValueError,KeyError,StopIteration):pass
  time.sleep(.05)
 raise AssertionError('Timed out; inspect '+str(out))
def start(args,env,name):
 log=(out/(name+'.log')).open('w');logs.append(log);p=subprocess.Popen(list(map(str,args)),env=env,stdout=log,stderr=log);children.append(p);return p
try:
 with tempfile.TemporaryDirectory(prefix='dome-resolution-') as directory:
  runtime=Path(directory);runtime.chmod(0o700)
  source=runtime/'probe.cpp';source.write_text(r'''#include <FL/Fl.H>
#include <FL/Fl_Double_Window.H>
#include <fstream>
#include <string>
#include <cstdio>
extern int fl_wl_buffer_scale(Fl_Window*);
static Fl_Double_Window *w,*dialog,*resizable_dialog,*nested_dialog;
static std::string report;
static void sample(void*) {
    std::ifstream resize(report+".resize");int width,height;
    if(resizable_dialog && resize >> width >> height) {
        resizable_dialog->size(width,height);std::remove((report+".resize").c_str());
    }
    std::ofstream f(report+".tmp");
    f << "{\"width\":" << w->w() << ",\"height\":" << w->h()
      << ",\"buffer_scale\":" << fl_wl_buffer_scale(w)
      << ",\"ui_scale\":" << Fl::screen_scale(w->screen_num())
      << ",\"dialog\":[" << (dialog?dialog->w():0) << "," << (dialog?dialog->h():0)
      << "],\"resizable_dialog\":[" << (resizable_dialog?resizable_dialog->w():0)
      << "," << (resizable_dialog?resizable_dialog->h():0)
      << "],\"nested_dialog\":[" << (nested_dialog?nested_dialog->w():0)
      << "," << (nested_dialog?nested_dialog->h():0) << "]}";
    f.close();std::rename((report+".tmp").c_str(),report.c_str());
    Fl::repeat_timeout(.1,sample);
}
static Fl_Double_Window *child_window(const char *title,const char *identity,bool resizable) {
    auto child = new Fl_Double_Window(resizable?1600:320,resizable?900:180,title);
    child->xclass(identity);child->set_modal();
    if(resizable)child->resizable(child);
    child->end();child->show();return child;
}
static void child(void*) {
    dialog=child_window("Resolution Dialog","org.domed.ResolutionDialog",false);
    resizable_dialog=child_window("Resizable Dialog","org.domed.ResizableDialog",true);
    nested_dialog=child_window("Nested Dialog","org.domed.NestedDialog",true);
}
int main(int argc,char**argv) {
    report=argv[1];bool fixed=std::string(argv[3])=="fixed";
    w=new Fl_Double_Window(fixed?640:720,fixed?480:460,"Resolution Probe");
    w->xclass(argv[2]);w->end();
    if(fixed)w->size_range(640,480,640,480);else w->resizable(w);
    w->show();Fl::add_timeout(.1,sample);
    if(!fixed)Fl::add_timeout(.4,child);
    return Fl::run();
}
''')
  subprocess.run(['g++','-O2','-I/opt/fltk-vr/toolkit/include',str(source),'-L/opt/fltk-vr/toolkit/lib','-Wl,-rpath,/opt/fltk-vr/toolkit/lib','-lfltk','-o',str(runtime/'probe'),*subprocess.check_output(['pkg-config','--cflags','cairo'],text=True).split()],check=True)
  rd,wr=os.pipe();xvfb=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','2560x1440x24','-nolisten','tcp'],pass_fds=(wr,),stderr=subprocess.DEVNULL);children.append(xvfb);os.close(wr)
  with os.fdopen(rd) as stream:display=':'+stream.readline().strip()
  with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
  env={**os.environ,'DISPLAY':display,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':directory+'/config','XDG_STATE_HOME':directory+'/state','XDG_CACHE_HOME':directory+'/cache','XDG_DATA_HOME':directory+'/data','XDG_CURRENT_DESKTOP':'Dome','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_START_XWAYLAND':'0','DOME_SESSION_RESTORE':'0','DOME_APPEARANCE_SERVICE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','FLTK_BACKEND':'wayland','LD_LIBRARY_PATH':'/opt/fltk-vr/toolkit/lib'}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
  for service in ['TRAY','KDECONNECT','KWALLET','POLKIT','IME']:env['DOME_START_'+service]='0'
  start([os.environ.get('DOME_RESOLUTION_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor')
  http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
  def request(path,body=None):
   with http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=3) as r:return json.load(r)
  def state():return request('/api/state')['scene']
  wait(state)
  records=wait(lambda:list(runtime.glob('*.dome-session.json')) if len(list(runtime.glob('*.dome-session.json')))>=2 else None)
  displays={json.loads(p.read_text())['realm']:p.name.removesuffix('.dome-session.json') for p in records}
  def probe(name,realm,fixed=False):
   path=out/(name+'.json');path.unlink(missing_ok=True);identity='org.domed.'+name
   app=start([runtime/'probe',path,identity,'fixed' if fixed else 'ordinary'],{**env,'WAYLAND_DISPLAY':displays[realm]},name)
   wait(lambda:path.exists());time.sleep(.8)
   result=json.loads(path.read_text());window=next(w for w in state()['windows'] if w['app_id']==identity)
   result['scene']=window['desktop'];results[name]=result
   return app,path,window['id']
  desktop,_,_=probe('Desktop','desktop');xr,path,ident=probe('XR','xr');fixed,_,_=probe('Fixed','xr',True)
  assert [results['Desktop']['width'],results['Desktop']['height']]==[720,460],results
  assert [results['XR']['width'],results['XR']['height']]==[1920,1080],results
  assert results['XR']['dialog']==[320,180],results
  assert results['XR']['resizable_dialog']==[960,540],results
  assert results['XR']['nested_dialog']==[960,540],results
  native_dialogs={w['app_id']:w['id'] for w in state()['windows'] if w.get('client_pid')==xr.pid}
  nested_id=native_dialogs['org.domed.NestedDialog']
  assert next(p for p in state()['popups'] if p['id']==nested_id)['parent']==native_dialogs['org.domed.ResizableDialog']
  assert results['Desktop']['resizable_dialog']==[1600,900],results
  # Use actual FLTK transient-parent descriptors and the optical XR layout.
  tracking={'id':771,'visible':True,'focused':True,
      'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},
      'horizontal_fov_deg':50,'mouse_viewport':[-.4,.47,-.32,.36]}
  request('/api/xr-session',tracking)
  def heartbeat():
   while not stop.wait(.1):
    try:request('/api/xr-session',tracking)
    except OSError:return
  threading.Thread(target=heartbeat,daemon=True).start()
  assert request('/api/xr-control')['centered_popups']
  geometry={q['id']:q for q in request('/api/xr-layout')['quads']}
  for identity in ['org.domed.ResolutionDialog','org.domed.ResizableDialog','org.domed.NestedDialog']:
   child_id=native_dialogs[identity]
   descriptor=next(p for p in state()['popups'] if p['id']==child_id)
   child=geometry[child_id];parent=geometry[descriptor['parent']]
   assert child['pose']['position_m'][1] < 1.4,(identity,child)
   # Native parent dialogs can already touch the optical edge. Verify the actual
   # rendered corners, where fitting may override the preferred parent offset.
   def cross(a,b):return [a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]]
   rotation=child['pose']['orientation_xyzw'];q=rotation[:3]
   for sx,sy in [(-1,-1),(-1,1),(1,-1),(1,1)]:
    corner=[sx*child['size'][0]/2,sy*child['size'][1]/2,0]
    t=[2*v for v in cross(q,corner)];c=cross(q,t)
    point=[child['pose']['position_m'][i]+corner[i]+rotation[3]*t[i]+c[i] for i in range(3)]
    point[1]-=1.4
    assert point[2]<0
    x,y=point[0]/-point[2],point[1]/-point[2]
    assert math.tan(-.4)<=x<=math.tan(.47) and math.tan(-.32)<=y<=math.tan(.36),(identity,point)
  print('PASS real FLTK fixed, resizable and nested dialogs stay below eye level and entirely inside narrow XR view',flush=True)
  assert [results['Fixed']['width'],results['Fixed']['height']]==[640,480],results
  assert (results['XR']['buffer_scale'],results['XR']['ui_scale'])==(results['Desktop']['buffer_scale'],results['Desktop']['ui_scale']),results
  density=state()['config']['xr_render_scale']
  request('/api/action',{'op':'desktop_resize','id':ident,'w':800,'h':500})
  wait(lambda:json.loads(path.read_text())['width']==800 and json.loads(path.read_text())['height']==500)
  Path(str(path)+'.resize').write_text('1280 800')
  wait(lambda:json.loads(path.read_text())['resizable_dialog']==[1280,800])
  assert state()['config']['xr_render_scale']==density
  print('PASS 1080p default, half-resolution XR/nested dialogs, unchanged desktop scale and fixed clients and later user resize',flush=True)
  for preset,expected in [('720p',[1280,720]),('900p',[1600,900]),('1440p',[2560,1440])]:
   config=state()['config'];config['xr_window_resolution']=preset
   request('/api/action',{'op':'configure','config':config});time.sleep(.15)
   assert state()['config']['xr_render_scale']==density
   assert [json.loads(path.read_text())['width'],json.loads(path.read_text())['height']]==[800,500]
   assert json.loads(path.read_text())['resizable_dialog']==[1280,800]
   _,_,_=probe('Preset'+preset,'xr')
   result=results['Preset'+preset]
   assert [result['width'],result['height']]==expected,result
   assert result['resizable_dialog']==[value//2 for value in expected],result
   assert result['nested_dialog']==[value//2 for value in expected],result
   assert result['dialog']==[320,180],result
   assert (result['buffer_scale'],result['ui_scale'])==(results['Desktop']['buffer_scale'],results['Desktop']['ui_scale']),result
  print('PASS 720p/900p/1080p/QHD presets apply only to new windows, preserving scale and existing sizes',flush=True)
  # Compare the actual installed VRCX executable with its desktop launch.
  if Path('/usr/bin/vrcx-0').exists():
   for realm in ['desktop','xr']:
    profile=runtime/('vrcx-'+realm)
    p=start(['/usr/bin/vrcx-0','--offline','--preview-read-only','--data-dir',profile],{**env,'WAYLAND_DISPLAY':displays[realm]},'vrcx-'+realm)
    w=wait(lambda:next((w for w in state()['windows'] if w.get('client_pid')==p.pid),None));time.sleep(.8)
    w=next(w for w in state()['windows'] if w.get('client_pid')==p.pid)
    results['vrcx-'+realm]=w['desktop']
    assert not w['maximized'] and w['xr_scale']==1
    if realm=='xr':assert [w['desktop']['w'],w['desktop']['h']]==[2560,1440],w
    p.terminate();p.wait(timeout=8)
   print('PASS isolated, offline VRCX desktop/XR launch comparison',flush=True)
  (out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
finally:
 stop.set()
 for child in reversed(children):
  if child.poll()is None:
   child.terminate()
   try:child.wait(timeout=5)
   except subprocess.TimeoutExpired:child.kill();child.wait()
 for log in logs:log.close()
