#!/usr/bin/env python3
"""GTK GMenu exports, legacy GTK module, and compositor popup policy on a private display.
Run with uv --with python-xlib --with pillow; /usr/bin/python3 needs GTK3 introspection.
DOME_TEST_SKIP_LEGACY=1 skips the optional appmenu-gtk-module fixture.
"""
import json, os, re, signal, socket, subprocess, sys, tempfile, time, urllib.request, threading
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_GTK_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_GTK_TEST_BUS':'1'})
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image
root=Path(__file__).resolve().parents[1];out=root/'artifacts/gtk-menus';out.mkdir(parents=True,exist_ok=True)
procs=[];logs=[];checks=[];heartbeat_stop=threading.Event();pulse=None
def start(args,name,env,**kw):
    f=(out/(name+'.log')).open('w');logs.append(f)
    p=subprocess.Popen(args,env=env,stdout=f,stderr=f,start_new_session=True,**kw);procs.append(p);return p
def wait(fn,timeout=20):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,StopIteration):pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/gtk-menus')
def check(ok,label):
    checks.append({'passed':bool(ok),'check':label});print(('PASS 'if ok else'FAIL ')+label,flush=True);assert ok,label
try:
    tmp=tempfile.mkdtemp(prefix='dome-gtk-')
    runtime=Path(tmp);runtime.chmod(0o700)
    apps=runtime/'data/applications';apps.mkdir(parents=True)
    for name,args in [('Native',''),('Legacy','--legacy')]+([('Native4','--gtk4')] if os.environ.get('DOME_TEST_GTK4') else []):
        command=f'/usr/bin/python3 {root}/scripts/gtk-menu-client.py {args}'
        if name=='Legacy':command='/usr/bin/env GTK_MODULES=appmenu-gtk-module UBUNTU_MENUPROXY=1 '+command
        wmclass={'Native':'org.domed.GtkMenuFixture','Native4':'org.domed.Gtk4MenuFixture','Legacy':'org.domed.GtkLegacyFixture'}[name]
        (apps/(({'Native':'org.domed.GtkMenuFixture','Native4':'org.domed.Gtk4MenuFixture'}.get(name,name))+'.desktop')).write_text(f'[Desktop Entry]\nType=Application\nName=GTK {name} fixture\nExec={command}\nStartupWMClass={wmclass}\n')
    rd,wr=os.pipe();start([os.environ.get('DOME_XVFB','Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd) as f:ds=':'+f.readline().strip()
    with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    env={**os.environ,'HOME':tmp,'XDG_CONFIG_HOME':tmp+'/config','XDG_DATA_HOME':tmp+'/data','XDG_STATE_HOME':tmp+'/state','XDG_RUNTIME_DIR':tmp,'DISPLAY':ds,'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_START_IME':'0','DOME_RENDERER':os.environ.get('DOME_TEST_RENDERER','gles'),'DOME_PREVIEW_PORT':str(port),'GTK_USE_PORTAL':'0','NO_AT_BRIDGE':'1','GIO_USE_VFS':'local'}
    for k in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','GTK_MODULES','UBUNTU_MENUPROXY']:env.pop(k,None)
    subprocess.run(['dbus-update-activation-environment','HOME','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','DISPLAY','QT_QPA_PLATFORM=offscreen'],env=env,check=True)
    compositor=start([os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],'compositor',env)
    base=f'http://127.0.0.1:{port}'
    def get(path):return json.load(urllib.request.urlopen(base+path,timeout=2))
    def scene():return get('/api/state')['scene']
    def post(path,value):
        urllib.request.urlopen(urllib.request.Request(base+path,data=json.dumps(value).encode(),headers={'Content-Type':'application/json'}),timeout=2).close()
    wait(lambda:scene()['source']=='wayland')
    catalog=get('/api/apps')
    post('/api/launch',{'id':next(a['id']for a in catalog if a['name']=='GTK Native fixture')})
    aid=wait(lambda:next((w['id']for w in scene()['windows']if w['title']=='GTK menu fixture'),None))
    check(True,'GTK application opens through DomeD desktop-entry launch on its Wayland socket')
    xd=display.Display(ds);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
    host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
    def motion(x,y):
        g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.15)
    def click(x,y):
        motion(x,y)
        for event in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(xd,event,1);xd.sync();time.sleep(.08)
    def key(name):
        code=xd.keysym_to_keycode(XK.string_to_keysym(name))
        for event in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,event,code);xd.sync();time.sleep(.08)
    def title(id):return next(w['title'] for w in scene()['windows']if w['id']==id)
    def shot(name):
        g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/(name+'.png'))
    time.sleep(1);click(260,18);time.sleep(.5);shot('gtk-global-menu')
    click(280,54);wait(lambda:title(aid)=='GTK activate activated')
    check(True,'Native GTK GMenu global menu invokes the exported GAction')
    click(260,18);click(280,82);time.sleep(.3)
    check(title(aid)=='GTK activate activated','Disabled GTK menu action cannot activate')
    key('Escape');click(260,18);click(280,110);wait(lambda:title(aid)=='GTK toggle True')
    check(True,'Stateful GTK check action updates its state')
    click(260,18);motion(280,138);time.sleep(.3);shot('gtk-global-submenu');click(740,150)
    wait(lambda:title(aid)=='GTK nested activated');check(True,'Nested GTK GMenu action works in universal menu')
    # MenuButton popups are not menubars and remain separate Wayland popup surfaces.
    w=next(w for w in scene()['windows']if w['id']==aid);r=w['desktop']
    click(r['x']+100,r['y']+80)
    popup=wait(lambda:next((p for p in scene()['popups']if p['root']==aid),None))
    check(abs(popup['anchor']['x']-(r['w']+8))<1,'GTK popup acknowledges DomeD side-panel placement in xdg configure')
    time.sleep(.3);shot('gtk-policy-popup')
    click(r['x']+popup['anchor']['x']+70,r['y']+popup['anchor']['y']+16)
    wait(lambda:title(aid)=='GTK popup activated');check(True,'GTK popup action accepts pointer input at DomeD placement')
    click(r['x']+100,r['y']+80);wait(lambda:scene()['popups'])
    click(260,18);click(280,54);wait(lambda:title(aid)=='GTK activate activated')
    check(True,'DomeD global menu takes priority over an active GTK popup grab')
    if os.environ.get('DOME_TEST_SKIP_LEGACY') != '1':
        native_before=title(aid)
        post('/api/launch',{'id':next(a['id']for a in catalog if a['name']=='GTK Legacy fixture')})
        bid=wait(lambda:next((w['id']for w in scene()['windows']if w['title']=='GTK legacy fixture'),None))
        post('/api/action',{'op':'focus','id':bid});time.sleep(3);shot('gtk-legacy-before');click(370,18);time.sleep(.5);shot('gtk-legacy-global-menu');click(370,54)
        wait(lambda:title(bid)=='GTK legacy activated');check(True,'Legacy GTK menu is exported by appmenu module and activated globally')
        check(title(aid)==native_before,'Focus change keeps GTK actions routed to their own application')
        post('/api/launch',{'id':next(a['id']for a in catalog if a['name']=='GTK Native fixture')})
        wait(lambda:scene()['focused']==aid)
        check(len(scene()['windows'])==2,'Launching an existing GTK application restores it through DomeD focus policy')
    else:
        print('SKIP legacy GTK exporter: module unavailable in test environment',flush=True)
        post('/api/action',{'op':'focus','id':aid})
    def heartbeat():
        while not heartbeat_stop.is_set():
            post('/api/xr-session',{'visible':True,'focused':True,'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':100})
            heartbeat_stop.wait(.1)
    pulse=threading.Thread(target=heartbeat,daemon=True);pulse.start()
    wait(lambda:get('/api/xr-control')['input_realm']=='xr');time.sleep(.4)
    post('/api/xr-input',{'events':[{'kind':'motion','id':aid,'uv':[.5,.5]}],'actions':[{'op':'focus','id':aid}]})
    wait(lambda:scene()['focused']==aid);time.sleep(.5)
    def xr_click(id,uv):
        post('/api/xr-input',{'events':[{'kind':'motion','id':id,'uv':uv}],'actions':[]});time.sleep(.15)
        post('/api/xr-input',{'events':[{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}],'actions':[]});time.sleep(.25)
    def xr_shell_click(x,y):
        layout=get('/api/xr-shell');xr_click(0,[x/layout['width'],y/layout['top']])
    xr_shell_click(260,18);xr_shell_click(280,110)
    wait(lambda:title(aid)=='GTK toggle False');check(True,'GTK global menu dispatch follows focused application in XR')
    r=next(w['desktop']for w in scene()['windows']if w['id']==aid)
    xr_click(aid,[100/r['w'],80/r['h']])
    popup=wait(lambda:next((p for p in scene()['popups']if p['root']==aid),None))
    check(popup['anchor']['y']<0,'GTK acknowledges DomeD above-parent popup placement in XR')
    time.sleep(.6) # Let GTK's popup activation guard expire before the next click.
    xr_click(popup['id'],[.3,.2]);wait(lambda:title(aid)=='GTK popup activated')
    check(True,'GTK floating popup accepts XR pointer input')
    heartbeat_stop.set();pulse.join(timeout=3)
    post('/api/xr-session',{'visible':False,'focused':False})
    wait(lambda:get('/api/xr-control')['input_realm']=='desktop');time.sleep(.3)
    if os.environ.get('DOME_TEST_GTK4'):
        post('/api/launch',{'id':next(a['id']for a in catalog if a['name']=='GTK Native4 fixture')})
        gid=wait(lambda:next((w['id']for w in scene()['windows']if w['title']=='GTK4 menu fixture'),None))
        time.sleep(3);click(260,18);time.sleep(.3);shot('gtk4-global-menu');click(290,54)
        wait(lambda:title(gid)=='GTK4 activated');check(True,'GTK4 GMenu exports and action dispatch use the universal menu')
        r=next(w['desktop']for w in scene()['windows']if w['id']==gid)
        click(r['x']+100,r['y']+80)
        popup=wait(lambda:next((p for p in scene()['popups']if p['root']==gid),None))
        check(abs(popup['anchor']['x']-(r['w']+8))<1,'GTK4 popover acknowledges DomeD popup placement')
        shot('gtk4-policy-popup');key('Escape')
    xd.close()
finally:
    heartbeat_stop.set()
    if pulse:pulse.join(timeout=3)
    for p in reversed(procs):
        try:os.killpg(p.pid,signal.SIGTERM)
        except ProcessLookupError:pass
    for p in reversed(procs):
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            try:os.killpg(p.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            p.wait()
    if 'tmp' in globals():
        subprocess.run(['fusermount3','-u',str(Path(tmp)/'doc')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        import shutil;shutil.rmtree(tmp,ignore_errors=True)
    for f in logs:f.close()
    (out/'results.json').write_text(json.dumps(checks,indent=2)+'\n')
