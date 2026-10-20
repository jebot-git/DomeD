#!/usr/bin/env python3
"""Real Qt menu export, focus routing, Vulkan composition and NumLock on a private display.
Build dome-compositor with --features standalone. Requires Qt development headers,
Xvfb, dbus-run-session and Mesa Zink. Run with uv --with python-xlib --with pillow.
Set DOME_QT6_PREFIX for extracted Qt headers, DOME_XVFB for a nonstandard Xvfb.
"""
import json, os, re, signal, socket, subprocess, sys, tempfile, time, urllib.request
from pathlib import Path
# A test launched from Dome may inherit the compositor's blocked termination signals.
signal.pthread_sigmask(signal.SIG_SETMASK, [])
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image, ImageChops

if os.environ.get('DOME_MENU_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv], {**os.environ, 'DOME_MENU_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/global-menu';out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[];checks=[];xd=None;host=None
for stale in out.glob('*.png'):stale.unlink()

def check(ok,label):
    checks.append({'passed':bool(ok),'check':label});print(('PASS ' if ok else 'FAIL ')+label,flush=True)
    assert ok,label

def wait(fn):
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,StopIteration):pass
        time.sleep(.05)
    raise AssertionError('Timed out; inspect artifacts/global-menu')

def start(args,name,env,**kwargs):
    f=(out/(name+'.log')).open('w');logs.append(f)
    p=subprocess.Popen(args,env=env,stdout=f,stderr=subprocess.STDOUT,start_new_session=True,**kwargs);processes.append(p);return p

def capture(name):
    geo=host.get_geometry();raw=host.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
    host_image=Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX');host_image.save(out/(name+'-host.png'))
    im=Image.open(out/'render.ppm').transpose(Image.Transpose.FLIP_TOP_BOTTOM);im.save(out/(name+'.png'))
    if name=='01-exported-menu':
        check(len(host_image.getcolors(10000000))>50 and host_image.getpixel((700,300))==im.getpixel((700,300)),
            'Vulkan-composited application pixels reach the actual host display')
    return im

def stop(p):
    try:os.killpg(p.pid,signal.SIGTERM);p.wait(timeout=4)
    except ProcessLookupError:pass
    except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()

try:
    temporary=tempfile.TemporaryDirectory(prefix='dome-global-menu-',ignore_cleanup_errors=True)
    directory=temporary.name
    runtime=Path(directory);runtime.chmod(0o700)
    prefix=Path(os.environ.get('DOME_QT6_PREFIX','/usr'))
    includes=prefix/'include/x86_64-linux-gnu/qt6'
    if not includes.exists():includes=prefix/'include/qt6'
    fixture=runtime/'menu-client'
    subprocess.run(['g++','-std=c++17','-fPIC','-I'+str(includes),str(root/'scripts/global-menu-client.cpp'),
        '/usr/lib/x86_64-linux-gnu/libQt6Widgets.so.6','/usr/lib/x86_64-linux-gnu/libQt6Gui.so.6','/usr/lib/x86_64-linux-gnu/libQt6Core.so.6','-o',str(fixture)],check=True)
    rd,wr=os.pipe();start([os.environ.get('DOME_XVFB','Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd) as pipe:display_name=':'+pipe.readline().strip()
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    config=runtime/'config';(config/'dome').mkdir(parents=True)
    (config/'dome/keyboard.json').write_text(json.dumps({'ime':False,'layout':'us'}))
    (config/'kcminputrc').write_text('[Keyboard]\nNumLock=0\n')
    env={**os.environ,'DISPLAY':display_name,'HOME':directory,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(config),
        'XDG_DATA_HOME':str(runtime/'data'),'XDG_CURRENT_DESKTOP':'Dome','XDG_SESSION_TYPE':'x11',
        'VK_DRIVER_FILES':os.environ.get('DOME_TEST_VULKAN_ICD','/usr/share/vulkan/icd.d/lvp_icd.json'),'LIBGL_ALWAYS_SOFTWARE':os.environ.get('DOME_TEST_SOFTWARE','1'),'LIBGL_KOPPER_DISABLE':'true','DOME_RENDER_CAPTURE':str(out/'render.ppm'),'DOME_LOGIN_SESSION':'0','DOME_PREVIEW_PORT':str(port),'DOME_RENDERER':'vulkan','DOME_START_XR':'0','RUST_LOG':'warn'}
    for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','QT_PLUGIN_PATH','LD_LIBRARY_PATH']:env.pop(key,None)
    subprocess.run(['dbus-update-activation-environment','HOME','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','DISPLAY','QT_QPA_PLATFORM=offscreen'],env=env,check=True)
    compositor=start([os.environ.get('DOME_COMPOSITOR',str(root/'target/debug/dome-compositor')),'--backend','nested'],'compositor',env)
    base=f'http://127.0.0.1:{port}'
    def scene():return json.load(urllib.request.urlopen(base+'/api/state',timeout=2))['scene']
    def action(data):
        urllib.request.urlopen(urllib.request.Request(base+'/api/action',data=json.dumps(data).encode(),headers={'Content-Type':'application/json'}),timeout=2).close();time.sleep(.3)
    def title(id):return next(w['title'] for w in scene()['windows'] if w['id']==id)
    wait(lambda:scene()['source']=='wayland')
    log=(out/'compositor.log').read_text()
    check('Dome compositor renderer: vulkan (zink Vulkan' in log,'Compositor uses Vulkan through Mesa Zink')
    wayland=re.search(r'Dome Wayland socket: (\S+)',log)[1]
    client_env={**env,'WAYLAND_DISPLAY':wayland,'QT_QPA_PLATFORM':'wayland','QT_WAYLAND_CLIENT_BUFFER_INTEGRATION':'shm'}
    client_env.pop('QT_QPA_PLATFORMTHEME',None)
    a=start([str(fixture),'first'],'first',client_env)
    aid=wait(lambda:next((w['id'] for w in scene()['windows'] if w['title']=='first'),None))
    xd=display.Display(display_name)
    host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if w.get_wm_name() and 'Dome' in w.get_wm_name()),None))
    host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.3)
    def motion(x,y):
        geo=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=x+geo.x,y=y+geo.y);xd.sync();time.sleep(.12)
    def click(x,y):
        motion(x,y)
        for event in [X.ButtonPress,X.ButtonRelease]:xtest.fake_input(xd,event,1);xd.sync();time.sleep(.06)
    def key(name):
        code=xd.keysym_to_keycode(XK.string_to_keysym(name));assert code,name
        for event in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,event,code);xd.sync();time.sleep(.06)
    action({'op':'focus','id':aid});time.sleep(1)
    key('KP_1');wait(lambda:title(aid)=='first-key-1')
    check(True,'Saved KDE NumLock=0 enables numeric keypad input at startup')
    key('Num_Lock');key('KP_1');check(title(aid)=='first-key-1','NumLock toggles keypad back to navigation')
    key('Num_Lock');key('KP_1');wait(lambda:title(aid)=='first-key-11')
    check(True,'NumLock can be toggled on again')
    click(260,18);time.sleep(.5);capture('01-exported-menu')
    click(280,54);wait(lambda:title(aid)=='first-activated-1')
    check(True,'Focused application exports its actual menu and receives activation')
    b=start([str(fixture),'second'],'second',client_env)
    bid=wait(lambda:next((w['id'] for w in scene()['windows'] if w['title']=='second'),None))
    action({'op':'focus','id':bid});time.sleep(.8)
    click(260,18);click(280,54);wait(lambda:title(bid)=='second-activated-1')
    check(title(aid)=='first-activated-1','Focus change dispatches menu actions only to the new application')
    click(260,18);click(280,82);time.sleep(.3)
    check(title(bid)=='second-activated-1','Disabled menu actions cannot activate')
    key('Escape');click(260,18);motion(280,138);time.sleep(.5);capture('02-nested-menu')
    click(640,150);wait(lambda:title(bid)=='second-nested-activated')
    check(True,'Nested application menu actions work')
    action({'op':'focus','id':aid});time.sleep(.8)
    click(330,18);click(340,54);time.sleep(.5);click(260,18);capture('03-updated-menu')
    before=Image.open(out/'01-exported-menu.png').crop((260,42,450,66))
    after=Image.open(out/'03-updated-menu.png').crop((260,42,450,66))
    check(ImageChops.difference(before,after).getbbox() is not None,'Application menu label updates repaint in the global bar')
    key('Down');key('Return');wait(lambda:title(aid)=='first-activated-2')
    check(True,'Keyboard navigation activates the focused exported menu')
    p=start([str(fixture),'plain'],'plain',client_env)
    pid=wait(lambda:next((w['id'] for w in scene()['windows'] if w['title']=='plain'),None))
    action({'op':'focus','id':pid});time.sleep(.8);capture('04-no-exported-menu')
    click(260,18);click(280,54);check(title(aid)=='first-activated-2' and title(bid)=='second-nested-activated','Non-exporting app does not expose stale application menus')
    check(compositor.poll() is None,'Compositor remains responsive after focus and menu changes')
finally:
    if xd is not None and host is not None:
        try:capture('final')
        except Exception:pass
    for p in reversed(processes):stop(p)
    for f in logs:f.close()
    if 'temporary' in globals():
        subprocess.run(['fusermount3','-u',str(Path(temporary.name)/'doc')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        temporary.cleanup()
    (out/'results.json').write_text(json.dumps(checks,indent=2)+'\n')
