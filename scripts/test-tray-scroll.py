#!/usr/bin/env python3
"""Real overflowing menu bars and dropdowns: desktop/XR input and remote IDs."""
import json, os, re, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_SYSTEM_MENU_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session','--',sys.executable,*sys.argv], {**os.environ,'DOME_SYSTEM_MENU_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=Path(os.environ.get('DOME_MENU_SCROLL_OUTPUT',str(root/'artifacts/tray-scroll')));out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];stop=threading.Event();checks=[]
for stale in out.glob('*.png'):stale.unlink()
(out/'failure-state.json').unlink(missing_ok=True)
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
    with tempfile.TemporaryDirectory(prefix='dome-tray-scroll-') as temporary:
        runtime=Path(temporary);runtime.chmod(0o700)
        source=runtime/'client.cpp'
        source.write_text('''#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <FL/Fl_Menu_Bar.H>
#include "desktop-menu.h"
#include <fstream>
#include <string>
static const char* marker;
static void activated(Fl_Widget*,void* value){std::ofstream(marker)<<reinterpret_cast<long>(value);}
int main(int argc,char**argv){marker=argv[1];Fl_Window w(480,320,"XR menu fixture");w.xclass("org.domed.SystemMenuFixture");
Fl_Menu_Bar m(0,0,480,28);for(long i=0;i<80;++i){std::string name="File/Action "+std::to_string(i);m.add(name.c_str(),0,activated,reinterpret_cast<void*>(i));}for(long i=0;i<20;++i){std::string name="Category "+std::to_string(i)+"/Activate";m.add(name.c_str(),0,activated,reinterpret_cast<void*>(200+i));}m.add("View/Application View",0,activated);m.add("Window/Application Window",0,activated);m.add("Help/Application Help",0,activated);
w.end();DesktopMenu exported(w,m);w.show();return Fl::run();}
''')
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
        compositor=start([os.environ.get('DOME_SYSTEM_MENU_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor')
        http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path,body=None):return http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=3)
        def read(path):
            with request(path) as response:return json.load(response)
        def state():return read('/api/state')['scene']
        def layout():return read('/api/xr-shell')
        wait(state)
        xr=wait(lambda:next((p for p in runtime.glob('*.dome-session.json') if json.loads(p.read_text())['realm']=='xr'),None))
        tracking={"position_m":[0,1.6,0],"orientation_xyzw":[0,0,0,1]}
        def heartbeat():
            while not stop.wait(.1):
                try:request('/api/xr-session',{'id':9182,'visible':True,'focused':True,'head':tracking,'horizontal_fov_deg':90,'mouse_viewport':[-.7,.7,-.65,.65]}).close()
                except OSError:pass
        thread=threading.Thread(target=heartbeat,daemon=True);thread.start()
        wait(lambda:read('/api/xr-control')['input_realm']=='xr')
        client=start([str(runtime/'client'),str(runtime/'native-marker')],{**env,'WAYLAND_DISPLAY':xr.name.removesuffix('.dome-session.json')},'client')
        window=wait(lambda:next((w for w in state()['windows'] if w['app_id']=='org.domed.SystemMenuFixture'),None));wid=window['id']
        def win():return next(w for w in state()['windows'] if w['id']==wid)
        request('/api/action',{'op':'focus','id':wid}).close();time.sleep(.3)
        xd=display.Display(screen);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
        def key(name,pressed=None):
            code=xd.keysym_to_keycode(XK.string_to_keysym(name))
            for kind in ((X.KeyPress,X.KeyRelease) if pressed is None else (X.KeyPress if pressed else X.KeyRelease,)):xtest.fake_input(xd,kind,code);xd.sync();time.sleep(.06)
        def click(x,y):
            shell=layout()
            panel = shell.get('pullout') is not None and y >= 36
            ident = 2**64-6 if panel else 0
            uv = [x/shell['width'],(y-36)/(min(shell['top'],shell['height']-60)-36)] if panel else [x/shell['width'],y/36]
            request('/api/xr-input',{'actions':[],'events':[{'kind':'motion','id':ident,'uv':uv},{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}]}).close();time.sleep(.25)
        def capture(name,bar=False):
            def frame():
                try:
                    with request('/api/xr-frame/'+str(2**64-6 if not bar and layout().get('pullout') else 0)) as response:
                        w=int(response.headers['X-Dome-Width']);h=int(response.headers['X-Dome-Height']);data=response.read()
                    return Image.frombytes('RGBA',(w,h),data)
                except OSError:return None
            image=wait(frame);image.save(out/(name+'.png'));return image
        def main_menu():
            click(20,18);wait(lambda:layout()['top']>36);time.sleep(.1)
        tray_journal=runtime/'tray-events.jsonl'
        start([sys.executable,str(root/'scripts/tray-client.py'),str(tray_journal),'--long'],env,'tray-client')
        wait(lambda:tray_journal.exists() and 'registered' in tray_journal.read_text());time.sleep(.8)
        def events():return [e for e in map(json.loads,tray_journal.read_text().splitlines()) if e['kind']=='event']
        def open_tray():
            click(layout()['width']-220-14,18);wait(lambda:layout()['top']>36)
            assert layout()['top']<=428, layout()
        def motion(x,y):
            shell=layout()
            ident=0 if y<36 else 2**64-6
            uv=[x/shell['width'],y/36 if y<36 else (y-36)/(shell['top']-36)]
            request('/api/xr-input',{'actions':[],'events':[{'kind':'motion','id':ident,'uv':uv}]}).close();time.sleep(.1)
        def hold_edge(x, seconds):
            end=time.monotonic()+seconds
            while time.monotonic()<end:motion(x,layout()['top']-8)
        open_tray();key('End');time.sleep(.3);capture('01-keyboard-end')
        x=layout()['width']-260
        click(x,layout()['top']-42)
        wait(lambda:events() and events()[-1]['id']==1553)
        check(True,'End reveals and clicks final source action 1553 inside a bounded XR tray panel')
        wait(lambda:layout()['top']==36)
        open_tray();key('End');key('Home');key('Down');key('Up');time.sleep(.2)
        capture('02-keyboard-home');click(x,40+24+14)
        wait(lambda:len(events())==2)
        check(events()[-1]['id']==1000,'Home/arrow selection returns to first action with correct pointer hit testing')
        open_tray();motion(x,layout()['top']-8)
        first=capture('03-edge-start');hold_edge(x,.7);second=capture('03-edge-progress')
        check(first.tobytes()!=second.tobytes(),'Tray edge offsets repaint while selection stays empty')
        hold_edge(x,10.5) # Identical XR ray samples must keep the edge timer active.
        capture('03-edge-scroll-end');click(x,layout()['top']-42)
        wait(lambda:len(events())==3)
        check(events()[-1]['id']==1553,'Stationary XR pointer on lower control scrolls all 80 actions and activates the final ID')
        open_tray();motion(x,100)
        request('/api/xr-input',{'actions':[],'events':[{'kind':'scroll','horizontal':0,'vertical':10000}]}).close();time.sleep(.3)
        capture('04-wheel-end');click(x,layout()['top']-42)
        wait(lambda:len(events())==4)
        check(events()[-1]['id']==1553,'XR scroll input uses the same bounded source-row mapping')
        open_tray();key('End');key('Escape');wait(lambda:layout()['top']==36)
        check(client.poll() is None,'Escape dismisses without closing the client')
        # The same checks apply to actual exported universal-menu items.
        marker=runtime/'native-marker'
        def open_file():
            click(275,18);wait(lambda:layout()['top']>36)
        open_file();key('End');key('Up');time.sleep(.3);capture('05-universal-keyboard-end')
        click(300,layout()['top']-98);wait(lambda:marker.exists())
        check(marker.read_text()=='79','Universal menu End reveals final exported action and maps pointer click correctly')
        marker.unlink();open_file();motion(300,layout()['top']-8)
        first=capture('06-universal-edge-start');hold_edge(300,.7);second=capture('06-universal-edge-progress')
        check(first.tobytes()!=second.tobytes(),'Universal-menu edge offsets repaint while selection stays empty')
        hold_edge(300,10.5)
        capture('06-universal-edge-end');click(300,layout()['top']-98);wait(lambda:marker.exists())
        check(marker.read_text()=='79','Stationary pointer edge scrolling reaches final universal-menu action')
        # Overflowing horizontal headings must reveal themselves before the tray.
        marker.unlink();open_file();before=capture('08-header-keyboard-start',bar=True)
        for _ in range(21):key('Right') # File, Edit, then twenty exported categories.
        after=capture('08-header-keyboard-end',bar=True)
        check(before.tobytes()!=after.tobytes(),'XR Right navigation repaints horizontal menu positions')
        tray_start=layout()['width']-220-5*28 # Four builtins plus the fixture tray item.
        right=tray_start-24
        selected=before.getpixel((275,2))
        check(all(after.getpixel((x,2))==selected for x in range(right-112,right)),
              'XR keyboard reveals the entire selected heading before the tray')
        key('Return');wait(lambda:marker.exists())
        check(marker.read_text()=='219','XR keyboard reaches and activates a heading initially hidden behind the tray')
        marker.unlink();click(20,18);key('Escape') # Fixed system button resets the strip.
        motion(tray_start-12,18);before=capture('09-header-edge-start',bar=True)
        time.sleep(.8);after=capture('09-header-edge-progress',bar=True)
        check(before.tobytes()!=after.tobytes(),'Stationary XR pointer scrolls the horizontal bar with no dropdown open')
        # Identical rays keep the timer alive; stop exactly at the final heading.
        for _ in range(100):motion(tray_start-12,18)
        capture('09-header-edge-end',bar=True)
        click(tray_start-24-196-56,18);key('Return');wait(lambda:marker.exists())
        check(marker.read_text()=='219','XR pointer reveals the final headings and clicks the correct exported category')
        # Desktop uses the same menu viewport and actual X11 host input.
        stop.set();thread.join(timeout=2)
        request('/api/xr-session',{'id':9182,'visible':False,'focused':False,'head':tracking}).close()
        wait(lambda:read('/api/xr-control')['input_realm']=='desktop')
        def desktop_click(x,y):
            g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=g.x+int(x),y=g.y+int(y))
            for kind in (X.ButtonPress,X.ButtonRelease):xtest.fake_input(xd,kind,1)
            xd.sync();time.sleep(.3)
        g=host.get_geometry()
        desktop_click(g.width-220-14,18);key('End');time.sleep(.2)
        raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
        Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/'07-desktop-tray-end.png')
        desktop_click(g.width-260,g.height-98)
        wait(lambda:len(events())==5)
        check(events()[-1]['id']==1553,'Desktop tray End reveals final action and preserves source-row hit testing')
        marker.unlink();desktop_click(20,18);key('Escape');desktop_click(275,18)
        for _ in range(21):key('Right')
        key('Return');wait(lambda:marker.exists())
        check(marker.read_text()=='219','Desktop keyboard reveals and activates the same overflowing exported heading')
        xd.close();(out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
except Exception:
    try:
        (out/'failure-state.json').write_text(json.dumps(read('/api/state'),indent=2))
        capture('failure')
    except Exception:pass
    raise
finally:
    stop.set()
    for child in reversed(children):
        if child.poll()is None:
            child.terminate()
            try:child.wait(timeout=4)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for stream in logs:stream.close()
