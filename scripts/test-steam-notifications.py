#!/usr/bin/env python3
"""Route real X11 Steam-style toast surfaces through the session-owned bridge.
Run: uv run --no-project --with python-xlib --with pillow python scripts/test-steam-notifications.py
All displays, notifications and applications are isolated from the user's session.
"""
import json,os,re,shutil,signal,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from Xlib import X,Xutil,display
from Xlib.ext import xtest
from PIL import Image
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_STEAM_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_STEAM_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/steam-notifications';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];checks=[]
def start(args,name,env,**kwargs):
    log=(out/(name+'.log')).open('w');logs.append(log)
    child=subprocess.Popen(args,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,**kwargs);children.append(child);return child
def wait(fn,timeout=12):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError):pass
        time.sleep(.05)
    raise AssertionError('Timed out: '+str(fn))
def check(value,name):
    checks.append({'check':name,'passed':bool(value)});print(('PASS 'if value else'FAIL ')+name,flush=True);assert value,name

def cleanup():
    for child in reversed(children):
        if child.poll()is None:child.terminate()
        try:child.wait(timeout=5)
        except subprocess.TimeoutExpired:child.kill();child.wait()

result=1
try:
  with tempfile.TemporaryDirectory(prefix='dome-steam-notices-')as directory:
    runtime=Path(directory);runtime.chmod(0o700)
    with socket.socket()as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    rd,wr=os.pipe();start([os.environ.get('DOME_XVFB')or shutil.which('Xvfb')or str(root/'.local-deps/root/usr/bin/Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as pipe:host_display=':'+pipe.readline().strip()
    env={**os.environ,'DISPLAY':host_display,'XDG_RUNTIME_DIR':directory,'HOME':str(runtime/'home'),
         'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),
         'XDG_DATA_DIRS':str(runtime/'empty'),'DOME_LOGIN_SESSION':'1','DOME_START_XR':'0','DOME_SESSION_RESTORE':'0',
         'DOME_START_XWAYLAND':'1','DOME_DRM_TEST_DEVICE':'/dev/null','DOME_APPEARANCE_SERVICE':'0',
         'DOME_PREVIEW_PORT':str(port),'DOME_RENDERER':os.environ.get('DOME_TEST_RENDERER','gles'),'LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
    for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','GTK_MODULES','QT_QPA_PLATFORM','PAM_KWALLET5_LOGIN']:env.pop(name,None)
    if env['DOME_RENDERER']=='vulkan':
        env.pop('LIBGL_ALWAYS_SOFTWARE',None);env['XDG_DATA_DIRS']+=':/usr/share'
    compositor=start([os.environ.get('DOME_COMPOSITOR',str(root/'target/debug/dome-compositor')),'--backend','nested'],'compositor',env)
    bridge=wait(lambda:re.search(r'Dome Xwayland bridge: (:\d+)',(out/'compositor.log').read_text()))[1]
    wait(lambda:Path('/tmp/.X11-unix/X'+bridge[1:]).exists())
    base=f'http://127.0.0.1:{port}'
    def get(path):return json.load(urllib.request.urlopen(base+path,timeout=2))
    def scene():return get('/api/state')['scene']
    def notice_frame():
        with urllib.request.urlopen(base+'/api/xr-frame/'+str(2**64-4),timeout=2)as r:
            return Image.frombytes('RGBA',(int(r.headers['X-Dome-Width']),int(r.headers['X-Dome-Height'])),r.read())
    x=display.Display(bridge);host=display.Display(host_display)
    host_window=wait(lambda:next((w for w in host.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
    host_window.set_input_focus(X.RevertToParent,X.CurrentTime);host.sync()
    def click(px,py):
        geometry=host_window.get_geometry();xtest.fake_input(host,X.MotionNotify,x=int(px+geometry.x),y=int(py+geometry.y))
        host.sync();time.sleep(.1)
        for event in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(host,event,1);host.sync();time.sleep(.1)
    def window(title,app='steam'):
        w=x.screen().root.create_window(0,0,283,70,0,x.screen().root_depth,X.InputOutput,X.CopyFromParent,
            background_pixel=0x102030,event_mask=X.ExposureMask|X.ButtonPressMask|X.ButtonReleaseMask|X.StructureNotifyMask)
        w.set_wm_name(title);w.set_wm_class('steamwebhelper',app)
        w.set_wm_normal_hints(flags=Xutil.PMinSize|Xutil.PMaxSize,min_width=283,min_height=70,max_width=283,max_height=70)
        w.map();x.sync();return w
    main=window('Steam');wait(lambda:any(w['title']=='Steam'for w in scene()['windows']))
    focused=scene()['focused']
    toast=window('notificationtoasts_123_desktop')
    gc=toast.create_gc(foreground=0xd64550);toast.fill_rectangle(gc,0,0,283,35)
    gc.change(foreground=0x2685b7);toast.fill_rectangle(gc,0,35,283,35);x.sync()
    wait(lambda:get('/api/xr-shell').get('notifications'))
    snapshot=scene();check(len(snapshot['windows'])==1 and snapshot['windows'][0]['title']=='Steam','Steam toast stays out of desktop/XR application and task lists')
    check(snapshot['focused']==focused,'Steam notification preserves the existing keyboard focus')
    def rendered_notice():
        gc.change(foreground=0xd64550);toast.fill_rectangle(gc,0,0,283,35)
        gc.change(foreground=0x2685b7);toast.fill_rectangle(gc,0,35,283,35);x.sync()
        image=notice_frame();colors=image.convert('RGB').getcolors(image.width*image.height)
        return image if any(n>1000 and c==(214,69,80)for n,c in colors)and any(n>1000 and c==(38,133,183)for n,c in colors)else None
    image=wait(rendered_notice);image.save(out/'steam-toast.png')
    colors=image.convert('RGB').getcolors(image.width*image.height)
    check(any(n>1000 and c==(214,69,80)for n,c in colors)and any(n>1000 and c==(38,133,183)for n,c in colors),'Notification server renders the captured X11 toast content into its XR surface')
    r=get('/api/xr-shell')['notifications'];r[0]=host_window.get_geometry().width-r[2]-12;click(r[0]+40,r[1]+50)
    def client_clicked():
        while x.pending_events():
            event=x.next_event()
            if event.type==X.ButtonPress and event.window.id==toast.id:return True
        return False
    check(wait(client_clicked),'Clicking the server toast is forwarded to the original Steam surface')
    check(scene()['focused']==focused,'Forwarded notification click keeps application keyboard focus')
    # Dismissal must suppress updates from this still-live native window.
    click(r[0]+r[2]-16,r[1]+16);wait(lambda:not get('/api/xr-shell').get('notifications'))
    gc.change(foreground=0x44aa55);toast.fill_rectangle(gc,0,0,283,70);x.sync();time.sleep(.4)
    check(not get('/api/xr-shell').get('notifications'),'Dismissed Steam toast does not reappear on client repaint')
    toast.unmap();x.sync();time.sleep(.3);toast.map();x.sync()
    wait(lambda:get('/api/xr-shell').get('notifications'))
    check(True,'A reused Steam toast window can deliver the next notification')
    toast.destroy();x.sync();wait(lambda:not get('/api/xr-shell').get('notifications'))
    check(True,'Destroying the Steam toast withdraws its server notification')
    reply=subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.Notifications',
        '--object-path','/org/freedesktop/Notifications','--method','org.freedesktop.Notifications.Notify',
        'Fixture','0','','Standard notification','D-Bus path remains available','[]','{}','0'],env=env,capture_output=True,text=True,check=True).stdout
    wait(lambda:get('/api/xr-shell').get('notifications'))
    check(get('/api/xr-shell')['notifications'][3]==100 and len(scene()['windows'])==1,'Standard D-Bus notifications still use the same server without a window')
    notice_id=re.search(r'uint32 (\d+)',reply)[1]
    subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.Notifications',
        '--object-path','/org/freedesktop/Notifications','--method','org.freedesktop.Notifications.CloseNotification',notice_id],env=env,capture_output=True,check=True)
    wait(lambda:not get('/api/xr-shell').get('notifications'))
    other=window('notificationtoasts_999_desktop','unrelated-app')
    wait(lambda:any(w['app_id']=='unrelated-app'for w in scene()['windows']))
    check(not get('/api/xr-shell').get('notifications'),'An unrelated application with the same title is not intercepted')
    other.destroy();main.destroy();x.sync();x.close();host.close();cleanup();result=0
finally:
    cleanup()
    for log in logs:log.close()
    (out/'report.json').write_text(json.dumps({'passed':result==0,'checks':checks},indent=2)+'\n')
sys.exit(result)
