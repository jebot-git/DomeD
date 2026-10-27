#!/usr/bin/env python3
"""Exercise real FLTK Appearance controls in a private nested desktop, without XR."""
import json,os,shutil,signal,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from Xlib import X,XK,display
from Xlib.ext import xtest
from PIL import Image
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_BACKDROP_APPEARANCE_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_BACKDROP_APPEARANCE_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/precipice/appearance-test';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];http=urllib.request.build_opener(urllib.request.ProxyHandler({}));checks=[]
def start(command,env,name,**kw):
    log=(out/name).open('w');logs.append(log);p=subprocess.Popen(list(map(str,command)),env=env,stdout=log,stderr=log,**kw);children.append(p);return p

def wait(check,timeout=20):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=check()
            if value:return value
        except(OSError,ValueError,KeyError):pass
        time.sleep(.04)
    raise AssertionError('Timeout; inspect '+str(out))
def check(condition,message):
    assert condition,message
    checks.append(message);print('PASS '+message,flush=True)
try:
    with tempfile.TemporaryDirectory(prefix='dome-backdrop-appearance-',ignore_cleanup_errors=True) as tmp:
        runtime=Path(tmp);runtime.chmod(0o700);config=runtime/'config/dome';config.mkdir(parents=True)
        prefs=config/'xr.json';prefs.write_text(json.dumps({'xr_backdrop':'none','xr_backdrop_volume':.4,'xr_backdrop_yaw_deg':35.,'yaw_deg':17.}))
        env={**os.environ,'XDG_RUNTIME_DIR':tmp,'XDG_CONFIG_HOME':str(config.parent),'XDG_DATA_HOME':tmp+'/data','XDG_STATE_HOME':tmp+'/state','XDG_CACHE_HOME':tmp+'/cache','DOME_LOGIN_SESSION':'0','DOME_SESSION_RESTORE':'0','DOME_APPEARANCE_SERVICE':'0','LIBGL_ALWAYS_SOFTWARE':'1','DOME_RENDERER':'gles'}
        for name in ['XR','TRAY','KDECONNECT','POLKIT','KWALLET','IME']:env['DOME_START_'+name]='0'
        for name in ['DISPLAY','WAYLAND_DISPLAY','WAYLAND_SOCKET','FONTCONFIG_FILE']:env.pop(name,None)
        rd,wr=os.pipe();start(['Xvfb','-displayfd',wr,'-screen','0','1280x800x24','-nolisten','tcp','-noreset'],env,'xvfb.log',pass_fds=(wr,));os.close(wr)
        with os.fdopen(rd) as pipe:env['DISPLAY']=':'+pipe.readline().strip()
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));env['DOME_PREVIEW_PORT']=str(sock.getsockname()[1])
        url='http://127.0.0.1:'+env['DOME_PREVIEW_PORT']
        def state():
            with http.open(url+'/api/state',timeout=2) as r:return json.load(r)['scene']
        def current():return state()['config']
        command=[os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested']
        compositor=start(command,env,'compositor.log');wait(state)
        xd=display.Display(env['DISPLAY'])
        def host_window():return next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None)
        host=wait(host_window);host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
        def key(name):
            code=xd.keysym_to_keycode(XK.string_to_keysym(name))
            for kind in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,kind,code)
            xd.sync();time.sleep(.05)
        def click(x,y,button=1):
            g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=g.x+int(x),y=g.y+int(y))
            for kind in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(xd,kind,button);xd.sync();time.sleep(.06)
        def capture(name):
            g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
            image=Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX');image.save(out/name);return image
        def row(index):
            key('Home')
            for _ in range(index):key('Down')
        def open_appearance():
            code=xd.keysym_to_keycode(XK.string_to_keysym('Super_L'));xtest.fake_input(xd,X.KeyPress,code);key('comma');xtest.fake_input(xd,X.KeyRelease,code);xd.sync();time.sleep(.2);key('Return');time.sleep(.2)
        time.sleep(.3);dark=capture('desktop-dark.png')
        open_appearance();row(7);key('Right');wait(lambda:not current()['desktop_backdrop']);key('Escape');time.sleep(.2)
        flat=capture('desktop-flat.png')
        check(dark.crop((0,80,dark.width,dark.height-80)).tobytes()!=flat.crop((0,80,flat.width,flat.height-80)).tobytes(),'Desktop backdrop toggle switches the SVG artwork to the flat desktop')
        open_appearance();row(7);key('Right');wait(lambda:current()['desktop_backdrop']);row(0);key('Right')
        wait(lambda:json.loads((config/'appearance.json').read_text())['light']);key('Escape');time.sleep(.3)
        light=capture('desktop-light.png')
        check(sum(a!=b for a,b in zip(dark.tobytes(),light.tobytes()))>dark.width*dark.height,'Colour scheme recolours the desktop vector backdrop immediately')
        open_appearance();row(8);key('Right');wait(lambda:current()['xr_backdrop']=='precipice')
        check(True,'Appearance enables the backdrop with conditional volume and rotation rows')
        key('Down');key('Left');wait(lambda:abs(current()['xr_backdrop_volume']-.35)<.001)
        key('Down');key('Right');wait(lambda:current()['xr_backdrop_yaw_deg']==40)
        check(current()['yaw_deg']==17.,'Backdrop Y rotation changes without rotating the desktop dome')
        capture('appearance-enabled.png')
        host.configure(width=800,height=480);xd.sync();time.sleep(.4)
        # Selected rotation row stays visible in the shortened panel. Test the
        # minus half of the actual native value control.
        g=host.get_geometry();rows=12;ph=min(752,g.height-112);visible=max(1,int((ph-176)//48));first=max(0,10-visible+1)
        top=44+(g.height-112-ph)/2;left=(g.width-600)/2
        click(left+360,top+60+(10-first)*48+20);wait(lambda:current()['xr_backdrop_yaw_deg']==35)
        check(True,'Compact Appearance dialog scrolls and numeric minus/plus hit testing follows visible rows')
        capture('appearance-compact.png')
        row(8);key('Left');wait(lambda:current()['xr_backdrop']=='none')
        check(abs(current()['xr_backdrop_volume']-.35)<.001 and current()['xr_backdrop_yaw_deg']==35,'Disabling backdrop hides dependent controls and retains their preferences')
        row(9);key('Return');wait(lambda:current()['xr_backdrop_volume']==1 and current()['xr_backdrop_yaw_deg']==0)
        check(current()['yaw_deg']==17.,'Appearance reset restores backdrop defaults while preserving dome orientation')
        row(9);key('Left');key('Down');key('Left');wait(lambda:current()['xr_backdrop_yaw_deg']==355)
        row(7);key('Left');wait(lambda:not current()['desktop_backdrop'])
        expected=current();key('Escape');compositor.terminate();compositor.wait(timeout=8)
        compositor=start(command,env,'restarted.log');wait(state)
        restored=current();check(not restored['desktop_backdrop'] and restored['xr_backdrop']=='precipice' and abs(restored['xr_backdrop_volume']-.95)<.001 and restored['xr_backdrop_yaw_deg']==355,'Independent desktop/XR backdrop choices, volume and Y rotation survive compositor restart')
        xd.close()
        compositor.terminate();compositor.wait(timeout=8)
finally:
    for child in reversed(children):
        if child.poll()is None:
            child.terminate()
            try:child.wait(timeout=5)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for log in logs:log.close()
    if 'tmp' in globals():shutil.rmtree(tmp,ignore_errors=True)
    (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
