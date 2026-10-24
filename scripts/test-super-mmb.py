#!/usr/bin/env python3
"""Super-MMB desktop snap and XR lane rotation through real XTest input."""
import json, os, re, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_SUPER_MMB_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session','--',sys.executable,*sys.argv], {**os.environ,'DOME_SUPER_MMB_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/super-mmb';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];stop=threading.Event();checks=[]
def wait(fn, seconds=15):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,StopIteration):pass
        time.sleep(.08)
    raise AssertionError('Timed out; inspect '+str(out))
def check(value,name):
    assert value,name
    checks.append(name);print('PASS '+name,flush=True)
def start(args,env,name):
    stream=(out/(name+'.log')).open('w');logs.append(stream)
    p=subprocess.Popen(args,env=env,stdout=stream,stderr=stream);children.append(p);return p
try:
    with tempfile.TemporaryDirectory(prefix='dome-system-menu-') as temporary:
        runtime=Path(temporary);runtime.chmod(0o700)
        source=runtime/'client.cpp'
        source.write_text('#include <FL/Fl.H>\n#include <FL/Fl_Window.H>\n#include <fstream>\nstatic const char *marker;\nclass Window:public Fl_Window {public:Window():Fl_Window(480,320,"Super MMB fixture"){}\nint handle(int e) override {if((e==FL_PUSH||e==FL_RELEASE)&&Fl::event_button()==2){std::ofstream(marker,std::ios::app)<<(e==FL_PUSH?"press\\n":"release\\n");return 1;}return Fl_Window::handle(e);}};\nint main(int argc,char **argv){marker=argv[1];Window w;w.xclass("org.domed.SuperMmbFixture");w.end();w.show();return Fl::run();}\n')
        protocol=root/'native/tray/appmenu.xml'
        subprocess.run(['wayland-scanner','client-header',str(protocol),str(runtime/'appmenu-client.h')],check=True)
        subprocess.run(['wayland-scanner','private-code',str(protocol),str(runtime/'appmenu-protocol.c')],check=True)
        flags=subprocess.check_output(['pkg-config','--cflags','--libs','gio-2.0','wayland-client','x11','cairo'],text=True).split()
        subprocess.run(['cc','-c',str(runtime/'appmenu-protocol.c'),'-o',str(runtime/'appmenu.o'),*flags],check=True)
        subprocess.run(['g++','-std=c++17','-O2','-I/opt/fltk-vr/toolkit/include','-I'+str(runtime),'-I'+str(root/'native/tray'),str(source),str(root/'native/tray/desktop-menu.cpp'),str(runtime/'appmenu.o'),'-L/opt/fltk-vr/toolkit/lib','-Wl,-rpath,/opt/fltk-vr/toolkit/lib','-lfltk',*flags,'-o',str(runtime/'client')],check=True)
        rd,wr=os.pipe();xvfb=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],pass_fds=(wr,),stderr=subprocess.DEVNULL);children.append(xvfb);os.close(wr)
        with os.fdopen(rd) as stream:screen=':'+stream.readline().strip()
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        env={**os.environ,'DISPLAY':screen,'XDG_RUNTIME_DIR':temporary,'XDG_CONFIG_HOME':temporary+'/config','XDG_STATE_HOME':temporary+'/state','XDG_CACHE_HOME':temporary+'/cache','XDG_DATA_HOME':temporary+'/data','XDG_CURRENT_DESKTOP':'Dome','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_APPEARANCE_SERVICE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','FLTK_BACKEND':'wayland','LD_LIBRARY_PATH':'/opt/fltk-vr/toolkit/lib'}
        for key in ('WAYLAND_DISPLAY','WAYLAND_SOCKET'):env.pop(key,None)
        for service in ('TRAY','KDECONNECT','KWALLET','POLKIT','IME'):env['DOME_START_'+service]='0'
        compositor=start([os.environ.get('DOME_SUPER_MMB_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor')
        http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path,body=None):return http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=3)
        def read(path):
            with request(path) as response:return json.load(response)
        def state():return read('/api/state')['scene']
        def layout():return read('/api/xr-shell')
        wait(state)
        desktop=wait(lambda:next((p for p in runtime.glob('*.dome-session.json') if json.loads(p.read_text())['realm']=='desktop'),None))
        marker=runtime/'middle-events'
        client=start([str(runtime/'client'),str(marker)],{**env,'WAYLAND_DISPLAY':desktop.name.removesuffix('.dome-session.json')},'client')
        window=wait(lambda:next((w for w in state()['windows'] if w['app_id']=='org.domed.SuperMmbFixture'),None));wid=window['id']
        def win():return next(w for w in state()['windows'] if w['id']==wid)
        xd=display.Display(screen);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.3)
        def key(name,pressed):
            code=xd.keysym_to_keycode(XK.string_to_keysym(name));xtest.fake_input(xd,X.KeyPress if pressed else X.KeyRelease,code);xd.sync();time.sleep(.08)
        def button(pressed):xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,2);xd.sync();time.sleep(.1)
        def move(x,y):
            g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.15)
        r=win()['desktop'];move(r['x']+100,r['y']+100);key('Super_L',True);button(True);move(r['x']+180,r['y']+150);key('Super_L',False);button(False)
        moved=wait(lambda:(w if (w:=win())['desktop']['x']>r['x']+70 else None))['desktop']
        check(abs(moved['x']-r['x']-80)<3 and abs(moved['y']-r['y']-50)<3,'Super-MMB drags desktop client content')
        check(not marker.exists(),'Reserved middle-button press and release do not leak to clients')
        move(moved['x']+100,moved['y']+100);key('Super_L',True);button(True);move(3,500);key('Super_L',False);button(False)
        snapped=wait(lambda:(w if (w:=win())['desktop']['x']<10 else None))['desktop'];g=host.get_geometry()
        check(abs(snapped['w']-(g.width/2-8))<3 and snapped['y']>=36 and snapped['y']+snapped['h']<=g.height-60,'Super-MMB edge snaps without Shift after releasing Super first')
        move(snapped['x']+120,snapped['y']+120);button(True);button(False)
        wait(lambda:marker.exists() and marker.read_text().splitlines()==['press','release'])
        check(True,'Ordinary middle-click is preserved')
        def heartbeat():
            while not stop.wait(.1):
                try:request('/api/xr-session',{'id':9183,'visible':True,'focused':True,'head':{'position_m':[0,1.6,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':90,'mouse_viewport':[-.7,.7,-.65,.65]}).close()
                except OSError:pass
        thread=threading.Thread(target=heartbeat,daemon=True);thread.start()
        wait(lambda:read('/api/xr-control')['input_realm']=='xr')
        request('/api/action',{'op':'focus','id':wid}).close();time.sleep(.2)
        original=state();workspace=original['active_workspace'];focus=original['focused'];before=original['workspaces']
        g=host.get_geometry();move(g.width/2,g.height/2);key('Super_L',True);button(True);move(g.width/2+100,g.height/2+30)
        rotated=wait(lambda:(s if abs((s:=state())['workspaces'][workspace]['lanes'][0]['rotation_deg']-before[workspace]['lanes'][0]['rotation_deg'])>5 else None))
        for lane in range(3):
            delta=(rotated['workspaces'][workspace]['lanes'][lane]['rotation_deg']-before[workspace]['lanes'][lane]['rotation_deg']+180)%360-180
            check(abs(delta-14.323945)<.3,'XR drag rotates lane '+str(lane)+' at pointer angular speed')
            check(rotated['workspaces'][workspace]['lanes'][lane]['windows']==before[workspace]['lanes'][lane]['windows'],'XR drag preserves lane '+str(lane)+' window order')
        check(rotated['focused']==focus and all(rotated['workspaces'][i]==w for i,w in enumerate(before) if i!=workspace),'XR rotation preserves focus and inactive workspaces')
        key('Super_L',False);button(False)
        settled=state()['workspaces'][workspace]['lanes'];move(g.width/2+140,g.height/2+30)
        check(state()['workspaces'][workspace]['lanes']==settled,'Releasing MMB ends rotation after Super was released first')
        check(marker.read_text().splitlines()==['press','release'],'XR reserved gesture does not leak middle-clicks')
        xd.close();(out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
finally:
    stop.set()
    for child in reversed(children):
        if child.poll()is None:
            child.terminate()
            try:child.wait(timeout=4)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for stream in logs:stream.close()
