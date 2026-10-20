#!/usr/bin/env python3
"""Exercise both built-in histories on a private D-Bus, Wayland and Xvfb seat.
Run: uv run --with dbus-next --with python-xlib --with pillow python scripts/test-shell-history.py
"""
import asyncio,json,os,re,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from dbus_next.aio import MessageBus
from Xlib import X,XK,display
from Xlib.ext import xtest
from PIL import Image,ImageChops
if os.environ.get('DOME_HISTORY_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_HISTORY_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/history';out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[]
def start(args,name,env,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log);p=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT,**kw);processes.append(p);return p

def wait(fn):
    until=time.monotonic()+10
    while time.monotonic()<until:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError):pass
        time.sleep(.04)
    raise AssertionError('Timed out')

def check(ok,label):
    print(('PASS 'if ok else'FAIL ')+label,flush=True);assert ok,label

async def main(directory):
    with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    base=f'http://127.0.0.1:{port}'
    def get(path):return json.load(urllib.request.urlopen(base+path,timeout=2))
    def packet(events):
        request=urllib.request.Request(base+'/api/xr-input',data=json.dumps({'events':events,'actions':[]}).encode(),headers={'Content-Type':'application/json'})
        with urllib.request.urlopen(request,timeout=2)as r:assert r.status==200
    rd,wr=os.pipe();start([str(root/'.local-deps/root/usr/bin/Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as f:xdisplay=':'+f.readline().strip()
    env={**os.environ,'DISPLAY':xdisplay,'XDG_RUNTIME_DIR':directory,'HOME':directory,'XDG_CONFIG_HOME':directory+'/config','XDG_DATA_HOME':directory+'/data','XDG_DATA_DIRS':directory+'/empty','XDG_SESSION_TYPE':'x11','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'0','LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
    for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','PAM_KWALLET5_LOGIN']:env.pop(name,None)
    Path(directory+'/config/dome').mkdir(parents=True);Path(directory+'/config/dome/keyboard.json').write_text('{"ime":false}')
    start([str(root/'target/debug/dome-compositor')],'compositor',env)
    wait(lambda:get('/api/state')['scene']['source']=='wayland')
    wayland=re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text())[1];client={**env,'WAYLAND_DISPLAY':wayland}
    xd=display.Display(xdisplay);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
    def key(name):
        code=xd.keysym_to_keycode(XK.string_to_keysym(name))
        for kind in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,kind,code);xd.sync();time.sleep(.03)
    def click(x,y):
        g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.07)
        for kind in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(xd,kind,1);xd.sync();time.sleep(.05)
    def shot(name=None):
        g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);image=Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX')
        if name:image.save(out/(name+'.png'))
        return image
    def copy(data,mime='text/plain;charset=utf-8'):
        p=start([str(root/'.local-deps/root/usr/bin/wl-copy'),'--foreground','--type',mime],'copy-'+str(len(processes)),client,stdin=subprocess.PIPE);p.stdin.write(data);p.stdin.close();time.sleep(.6)
    def paste(mime='text/plain;charset=utf-8'):
        return subprocess.run([str(root/'.local-deps/root/usr/bin/wl-paste'),'--no-newline','--type',mime],env=client,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=3).stdout
    first='First clipboard — Unicode ✓'.encode();copy(first);copy(b'Second clipboard item')
    click(1018,18);time.sleep(.15);shot('clipboard-history');check(get('/api/xr-shell')['top']==768,'Clipboard tray opens a readable history pane for XR too')
    key('Home');key('Down');key('Return');check(wait(lambda:paste()==first),'Selecting clipboard history restores exact Unicode bytes')
    data=bytes(range(256))*4;copy(data,'application/octet-stream');copy(b'Text after binary')
    click(1018,18);key('Home');key('Down');key('Return');check(wait(lambda:paste('application/octet-stream')==data),'Clipboard history restores the original binary MIME payload')
    click(1018,18);key('End');key('Delete');shot('clipboard-delete');click(400,640);time.sleep(.15);shot('clipboard-cleared');key('Escape')
    bus=await MessageBus().connect()
    for _ in range(100):
        try:description=await bus.introspect('org.freedesktop.Notifications','/org/freedesktop/Notifications');break
        except Exception:await asyncio.sleep(.05)
    notices=bus.get_proxy_object('org.freedesktop.Notifications','/org/freedesktop/Notifications',description).get_interface('org.freedesktop.Notifications');closed=[];notices.on_notification_closed(lambda id,reason:closed.append((id,reason)))
    id=await notices.call_notify('History test',0,'','Remember this notification','This remains in history after the toast expires',[],{},100)
    for _ in range(100):
        if (id,1)in closed:break
        await asyncio.sleep(.03)
    check((id,1)in closed,'Notification toast expires normally')
    before=shot();click(1046,18);await asyncio.sleep(.15);shown=shot('notification-history');check(ImageChops.difference(before,shown).getbbox()is not None,'Notification tray opens the retained notification history')
    key('Delete');await asyncio.sleep(.1);empty=shot('notification-deleted');check(ImageChops.difference(shown.crop((270,85,1000,145)),empty.crop((270,85,1000,145))).getbbox()is not None,'Expired notification remains in history until explicitly removed')
    key('Escape')
    # The same pane and clear control also work through the XR seat bridge.
    packet([{'kind':'motion','id':0,'uv':[790/1024,.5]},{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}]);await asyncio.sleep(.15)
    check(get('/api/xr-shell')['top']==768,'XR tray input opens the notification history pane')
    key('Escape');xd.close();bus.disconnect()

try:
    with tempfile.TemporaryDirectory(prefix='dome-history-')as directory:os.chmod(directory,0o700);asyncio.run(main(directory))
finally:
    for p in reversed(processes):
        if p.poll()is None:p.terminate()
    for p in reversed(processes):
        try:p.wait(timeout=3)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    for log in logs:log.close()
