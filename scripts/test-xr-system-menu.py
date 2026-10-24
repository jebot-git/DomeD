#!/usr/bin/env python3
"""XR shell menu controls with a native FLTK client on a private compositor."""
import json, os, re, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_SYSTEM_MENU_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session','--',sys.executable,*sys.argv], {**os.environ,'DOME_SYSTEM_MENU_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/xr-system-menu';out.mkdir(parents=True,exist_ok=True)
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
        source.write_text('''#include <FL/Fl.H>
#include <FL/Fl_Window.H>
#include <FL/Fl_Menu_Bar.H>
#include "desktop-menu.h"
#include <fstream>
static const char* marker;
static void activated(Fl_Widget*,void*){std::ofstream(marker)<<"native action";}
int main(int argc,char**argv){marker=argv[1];Fl_Window w(480,320,"XR menu fixture");w.xclass("org.domed.SystemMenuFixture");
Fl_Menu_Bar m(0,0,480,28);m.add("File/Native File",0,activated);m.add("View/Application View",0,activated);m.add("Window/Application Window",0,activated);
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
        def capture(name):
            def frame():
                try:
                    with request('/api/xr-frame/'+str(2**64-6 if layout().get('pullout') else 0)) as response:
                        w=int(response.headers['X-Dome-Width']);h=int(response.headers['X-Dome-Height']);data=response.read()
                    return Image.frombytes('RGBA',(w,h),data)
                except OSError:return None
            wait(frame).save(out/(name+'.png'))
        def main_menu():
            click(20,18);wait(lambda:layout()['top']>36);time.sleep(.1)
        # A native application View category replaces fallback View; Dome commands
        # must remain available from the compositor's own main menu.
        main_menu();capture('01-main-menu')
        opened=layout()['pullout']
        world=read('/api/xr-layout')
        panel_before=next(q for q in world['shell'] if q['id']==2**64-6)
        bar_before=next(q for q in world['shell'] if q['id']==0)
        assert opened['top_attached'] and panel_before['dome']['frame']==bar_before['dome']['frame']
        assert panel_before['dome']['top']<bar_before['dome']['bottom']
        tracking.update(position_m=[.6,1.9,-.4],orientation_xyzw=[0,.2955202,0,.9553365]);time.sleep(.4)
        assert layout()['pullout']==opened
        panel_after=next(q for q in read('/api/xr-layout')['shell'] if q['id']==2**64-6)
        bar_after=next(q for q in read('/api/xr-layout')['shell'] if q['id']==0)
        assert panel_after['dome']['frame']==bar_after['dome']['frame'] and panel_before!=panel_after
        with request('/api/xr-frame/0') as response:assert int(response.headers['X-Dome-Height'])==36
        check(True,'Universal-menu dropdown stays below and follows its top-bar origin, preserving the bar texture')
        tracking.update(position_m=[0,1.6,0],orientation_xyzw=[0,0,0,1]);time.sleep(.2)
        key('Escape');wait(lambda:layout()['top']==36)
        key('Super_L',True);key('F10');key('Super_L',False)
        wait(lambda:layout()['top']>36)
        assert not layout()['pullout']['top_attached']
        keyboard_before=next(q for q in read('/api/xr-layout')['shell'] if q['id']==2**64-6)
        tracking.update(position_m=[.6,1.9,-.4],orientation_xyzw=[0,.2955202,0,.9553365]);time.sleep(.4)
        keyboard_after=next(q for q in read('/api/xr-layout')['shell'] if q['id']==2**64-6)
        assert keyboard_before==keyboard_after
        check(True,'Keyboard-origin menu retains frozen pullout placement')
        key('Escape');wait(lambda:layout()['top']==36)
        tracking.update(position_m=[0,1.6,0],orientation_xyzw=[0,0,0,1]);time.sleep(.2)
        main_menu()
        click(160,40+4+24+5*28+14)
        shell=wait(lambda:(l if (l:=layout())['top']==l['height'] else None));capture('02-settings')
        w,h=shell['width'],shell['height'];pw=min(800,w-16);ph=min(650,h-100);left=(w-pw)/2;top=36+max(0,h-96-ph)/2
        click(left+150,top+40+34+17);capture('03-keyboard-layouts')
        click((w-600)/2+450,(h-400)/2+364)
        wait(lambda:layout()['top']==36)
        check(True,'XR main menu opens Settings and its Keyboard Layouts page with an application View menu active')
        main_menu();click(160,40+4+24+6*28+14)
        wait(lambda:layout()['top']==layout()['height']);capture('04-keybinding-help')
        previous=state()['config']['keybindings']
        click(left+pw-100,top+57);capture('05-binding-capture');key('Super_L',True);key('F9');key('Super_L',False)
        wait(lambda:state()['config']['keybindings'][0]['key']==0xffc6)
        updated=state()['config']['keybindings'];check(updated[1:]==previous[1:],'Keybinding Help starts at the first binding and pointer hit testing edits that exact binding')
        click(left+pw-80,top+ph-48);wait(lambda:layout()['top']==36)
        click(425,18);wait(lambda:layout()['top']>36);capture('05-native-window-menu')
        band=24 if layout()['top']<13*28+44 else 0
        click(480,36+4+band+14)
        wait(lambda:win()['desktop_visible'] and not win()['xr_visible'])
        check(client.poll()is None and not (runtime/'native-marker').exists(),'Universal Window Send to Desktop precedes native Window actions and transfers the running XR application')
        xd.close()
        (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
finally:
    stop.set()
    for child in reversed(children):
        if child.poll()is None:
            child.terminate()
            try:child.wait(timeout=4)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for stream in logs:stream.close()
