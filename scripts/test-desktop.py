#!/usr/bin/env python3
"""End-to-end Rust desktop tests on a disposable Xvfb display.
Uses Qt solely as native Wayland client fixtures, XTEST for test-seat events, and
framebuffer pixel assertions. Does not connect to or control the user's display.
Run: uv run --no-project --with PySide6 --with python-xlib --with pillow python scripts/test-desktop.py
Build dome-compositor with --features nested first. Requires Xvfb, Mesa and gio.
"""
import json,os,re,shutil,socket,subprocess,sys,tempfile,time,urllib.request,threading,signal
from pathlib import Path
import PySide6
from PIL import Image
from Xlib import X,XK,display
from Xlib.ext import xtest
if os.environ.get('DOME_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_TEST_BUS':'1'})
qt_lib=str(Path(PySide6.__file__).parent/'Qt/lib')
root=Path(__file__).resolve().parents[1];out=root/'artifacts/desktop';out.mkdir(parents=True,exist_ok=True)
(out/'report.json').unlink(missing_ok=True)
(out/'failure.png').unlink(missing_ok=True)
checks=[];processes=[];logs=[];xd=None

def check(ok,name,details=None):
    checks.append({'passed':bool(ok),'check':name,'details':details});print(('PASS 'if ok else'FAIL ')+name,flush=True)
    if not ok:raise AssertionError(name+': '+str(details))

def wait(fn,timeout=8):
    end=time.monotonic()+timeout;last=None
    while time.monotonic()<end:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError)as e:last=e
        time.sleep(.04)
    raise AssertionError('Timed out: '+str(last))

def start(args,name,env,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log)
    p=subprocess.Popen(args,env=env,cwd=root,stdout=log,stderr=subprocess.STDOUT,**kw);processes.append(p);return p

def events(name):
    p=out/(name+'.jsonl')
    if not p.exists():return []
    result=[]
    for line in p.read_text().splitlines():
        try:result.append(json.loads(line))
        except ValueError:pass
    return result

with socket.socket()as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
base=f'http://127.0.0.1:{port}'
def state():return json.load(urllib.request.urlopen(base+'/api/state',timeout=2))['scene']
def win(id):return next(w for w in state()['windows']if w['id']==id)
def focused_title():
    snapshot=state();return next((w['title'] for w in snapshot['windows'] if w['id']==snapshot['focused']),None)
def action(data):
    with urllib.request.urlopen(urllib.request.Request(base+'/api/action',data=json.dumps(data).encode(),headers={'Content-Type':'application/json'}),timeout=2)as r:assert r.status==200
    time.sleep(.2)

def motion(x,y):
    geometry=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(x+geometry.x),y=int(y+geometry.y));xd.sync();time.sleep(.08)
def button(pressed=True,code=1):
    xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,code);xd.sync();time.sleep(.06)
def click(x,y):motion(x,y);button();button(False);time.sleep(.1)
def key(name,pressed=None):
    symbol=XK.string_to_keysym(name);code=xd.keysym_to_keycode(symbol)
    assert code,'Unknown test key '+name
    for down in ([True,False]if pressed is None else[pressed]):xtest.fake_input(xd,X.KeyPress if down else X.KeyRelease,code);xd.sync();time.sleep(.025)
def xr_shortcut(modifiers,letter):
    for modifier in modifiers:key(modifier,True)
    key(letter)
    for modifier in reversed(modifiers):key(modifier,False)
def text(value):
    for letter in value:key('space'if letter==' 'else letter)
def drag(x,y,dx,dy):
    motion(x,y);button()
    for i in range(1,11):motion(x+dx*i/10,y+dy*i/10)
    button(False);time.sleep(.3)
def image():
    geometry=host.get_geometry();raw=host.get_image(0,0,geometry.width,geometry.height,X.ZPixmap,0xffffffff)
    return Image.frombytes('RGB',(geometry.width,geometry.height),raw.data,'raw','BGRX')
def capture(name):image().save(out/(name+'.png'))
def pixel(x,y):return image().getpixel((int(x),int(y)))
def color_at(x,y,target):
    c=pixel(x,y);return all(abs(a-b)<6 for a,b in zip(c,target))
red=(189,62,78);blue=(50,100,200)
result=1
try:
  with tempfile.TemporaryDirectory(prefix='dome-desktop-')as directory:
    runtime=Path(directory);runtime.chmod(0o700);data=runtime/'data';apps=data/'applications';apps.mkdir(parents=True);(runtime/'empty').mkdir()
    for name in ['a','b','launched']:(out/(name+'.jsonl')).unlink(missing_ok=True)
    # Only the disposable client is advertised by the launcher's desktop-entry search.
    (apps/'dome-fixture.desktop').write_text('[Desktop Entry]\nType=Application\nName=Dome Fixture Launcher\nExec=/usr/bin/env LD_LIBRARY_PATH='+qt_lib+':'+os.environ.get('LD_LIBRARY_PATH','')+' '+sys.executable+' '+str(root/'scripts/desktop-client.py')+' --name dome-launched --app-id dome-fixture --color "#36a381" --journal '+str(out/'launched.jsonl')+'\nTerminal=false\nIcon=application-x-executable\nCategories=Utility;\nKeywords=fixture;dome;\nActions=new;\n[Desktop Action new]\nName=New Test Window\nExec=/usr/bin/env LD_LIBRARY_PATH='+qt_lib+':'+os.environ.get('LD_LIBRARY_PATH','')+' '+sys.executable+' '+str(root/'scripts/desktop-client.py')+' --name dome-desktop-action --app-id dome-fixture --color \"#36a381\" --journal '+str(out/'desktop-action.jsonl')+'\n')
    (apps/'dome-xr-probe.desktop').write_text('[Desktop Entry]\nType=Application\nName=XR Recovery Probe\nExec=/usr/bin/env LD_LIBRARY_PATH='+qt_lib+':'+os.environ.get('LD_LIBRARY_PATH','')+' '+sys.executable+' '+str(root/'scripts/desktop-client.py')+' --name dome-xr-only --app-id dome-xr-probe --color "#36a381" --journal '+str(out/'xr-only.jsonl')+'\nTerminal=false\n')
    (out/'xr-only.jsonl').unlink(missing_ok=True)
    rd,wr=os.pipe()
    xvfb=os.environ.get('DOME_XVFB',shutil.which('Xvfb')or'')
    if not xvfb:raise RuntimeError('Xvfb is required')
    start([xvfb,'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as pipe:display_name=':'+pipe.readline().strip()
    assert display_name!=':'
    env={**os.environ,'DISPLAY':display_name,'XDG_RUNTIME_DIR':directory,'XDG_SESSION_TYPE':'x11','XDG_CONFIG_HOME':str(runtime/'config'),'XDG_CURRENT_DESKTOP':'Dome','XDG_DATA_HOME':str(data),'XDG_DATA_DIRS':str(runtime/'empty'),'DOME_PREVIEW_PORT':str(port),'LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
    for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','QT_PLUGIN_PATH','QT_QPA_PLATFORM_PLUGIN_PATH']:env.pop(name,None)
    env['DBUS_SYSTEM_BUS_ADDRESS']=env['DBUS_SESSION_BUS_ADDRESS']
    env['XDG_SEAT_PATH']='/org/freedesktop/DisplayManager/Seat0'
    (out/'session.jsonl').unlink(missing_ok=True)
    session_fixture=start([sys.executable,'scripts/session-client.py',str(out/'session.jsonl')],'session-fixture',env)
    wait(lambda:any(e['kind']=='ready' for e in events('session')))
    compositor=start([str(root/'target/debug/dome-compositor')],'compositor',env)
    wait(lambda:state()['source']=='wayland')
    match=re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text());assert match
    client_env={**env,'WAYLAND_DISPLAY':match[1],'QT_QPA_PLATFORM':'wayland','QT_WAYLAND_CLIENT_BUFFER_INTEGRATION':'shm','LD_LIBRARY_PATH':qt_lib+':'+env.get('LD_LIBRARY_PATH','')}
    a=start([sys.executable,'scripts/desktop-client.py','--name','dome-a','--color','#bd3e4e','--journal',str(out/'a.jsonl')],'client-a',client_env)
    wait(lambda:len(state()['windows'])==1);aid=state()['windows'][0]['id']
    b=start([sys.executable,'scripts/desktop-client.py','--name','dome-b','--color','#3264c8','--journal',str(out/'b.jsonl')],'client-b',client_env)
    wait(lambda:len(state()['windows'])==2);bid=next(w['id']for w in state()['windows']if w['id']!=aid)
    xd=display.Display(display_name);xroot=xd.screen().root
    def host_window():
        for window in xroot.query_tree().children:
            if str(window.get_wm_name()or'').startswith('Dome'):return window
        return None
    host=wait(host_window);host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.5)
    check(host.get_geometry().width==1280 and host.get_geometry().height==800,'Nested Rust output is 1280×800')
    check(wait(lambda:color_at(350,300,blue)),'GPU framebuffer contains the upper blue Wayland surface')
    capture('01-stacked')
    (out/'tray.jsonl').unlink(missing_ok=True)
    tray=start([sys.executable,'scripts/tray-client.py',str(out/'tray.jsonl')],'tray-client',env)
    wait(lambda:any(e['kind']=='registered'for e in events('tray')));time.sleep(.4)
    click(1090,18);wait(lambda:any(e['kind']=='layout'for e in events('tray')));capture('01-classic-status-menu')
    click(1040,54);wait(lambda:any(e['kind']=='event'and e['id']==10 and e['event']=='clicked'for e in events('tray')))
    check(True,'Classic right-side status menu invokes a real StatusNotifier DBusMenu action')
    tray.terminate();tray.wait(timeout=3);time.sleep(.3)
    check(all(abs(a-b)<2 for a,b in zip(pixel(300,2),pixel(1260,742))),'Global menu and taskbar share their background colour and opacity')
    click(20,18);click(100,166);capture('01-appearance-dark')
    click(800,280)
    settings=runtime/'config/dome/appearance.json'
    wait(lambda:settings.exists() and json.loads(settings.read_text())['light'])
    capture('01-appearance-light')
    check(all(abs(a-b)<2 for a,b in zip(pixel(650,10),pixel(1260,742))),'Light appearance updates both bars to the same palette')
    click(800,520);click(640,568)
    check(not json.loads(settings.read_text())['light'],'Appearance reset persists default settings')
    for row,kind in [(9,'lock'),(10,'switch_user')]:
        click(20,18);click(100,40+row*28+14);wait(lambda:any(e['kind']==kind for e in events('session')))
    check(True,'Session lock and switch-user commands reach private D-Bus fixtures')
    for row,kind in [(11,'suspend'),(13,'restart'),(14,'shutdown')]:
        click(20,18);click(100,40+row*28+14);capture('01-confirm-'+kind)
        before=len(events('session'));click(500,460);time.sleep(.15)
        check(not any(e['kind']==kind for e in events('session')[before:]),kind+' cancellation sends no session request')
        click(20,18);click(100,40+row*28+14);click(760,460)
        wait(lambda:any(e['kind']==kind and e['interactive'] for e in events('session')))
    check(True,'Confirmed sleep/restart/shutdown reach private logind fixtures only')
    # Native XR texture and real seat bridge, including release-on-timeout.
    import urllib.error
    def xr_packet(events,actions=[]):
        with urllib.request.urlopen(urllib.request.Request(base+'/api/xr-input',data=json.dumps({'events':events,'actions':actions}).encode(),headers={'Content-Type':'application/json'}),timeout=2)as response:assert response.status==200
    def xr_frame():
        try:
            with urllib.request.urlopen(base+f'/api/xr-frame/{bid}',timeout=2)as response:
                w,h=int(response.headers['X-Dome-Width']),int(response.headers['X-Dome-Height']);return Image.frombytes('RGBA',(w,h),response.read())
        except urllib.error.HTTPError as error:
            if error.code==409:return None
            raise
    texture=wait(xr_frame);texture.save(out/'01-xr-client-texture.png')
    check(texture.size==(int(win(bid)['desktop']['w']),int(win(bid)['desktop']['h'])),'XR texture contains client content without desktop Qt6 decoration borders')
    check(all(abs(a-b)<6 for a,b in zip(texture.getpixel((350,300))[:3],blue)),'XR readback contains live blue Wayland client pixels')
    white=lambda area:sum(min(p[:3])>180 for p in texture.crop(area).getdata())
    check(white((0,0,texture.width,80))>white((0,texture.height-80,texture.width,texture.height)),'XR texture preserves upright client text')
    for linux,qt in [(0x110,1),(0x112,4),(0x111,2)]:
        before=len(events('b'));xr_packet([{'kind':'motion','id':bid,'uv':[.5,.5]},{'kind':'button','button':linux,'pressed':True}], [{'op':'focus','id':bid}]);wait(lambda:any(e['kind']=='press'and e['button']==qt for e in events('b')[before:]))
        # A disconnected sender must release its held button without another packet.
        wait(lambda:any(e['kind']=='release'and e['button']==qt for e in events('b')[before:]))
    check(True,'XR LMB/MMB/RMB reach real Wayland seat and timeout releases every held button')
    shell=lambda:json.load(urllib.request.urlopen(base+'/api/xr-shell'))
    layout=wait(lambda:shell()if shell()['width']else None)
    xr_packet([{'kind':'motion','id':0,'uv':[420/layout['width'],18/layout['top']]},{'kind':'button','button':0x110,'pressed':True},{'kind':'button','button':0x110,'pressed':False}])
    wait(lambda:shell()['top']>36);layout=shell()
    xr_packet([{'kind':'motion','id':0,'uv':[430/layout['width'],54/layout['top']]},{'kind':'button','button':0x110,'pressed':True},{'kind':'button','button':0x110,'pressed':False}])
    wait(lambda:win(bid)['minimized']);action({'op':'focus','id':bid})
    check(True,'XR shell pinch opens the universal menu and invokes native Minimize')
    def post_json(path,value):
        with urllib.request.urlopen(urllib.request.Request(base+path,data=json.dumps(value).encode(),headers={'Content-Type':'application/json'}),timeout=2)as response:return json.load(response)
    def control():return json.load(urllib.request.urlopen(base+'/api/xr-control'))
    heartbeat_stop=threading.Event()
    def heartbeat():
        while not heartbeat_stop.is_set():
            post_json('/api/xr-session',{'visible':True,'focused':True,'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':100})
            heartbeat_stop.wait(.1)
    pulse=threading.Thread(target=heartbeat,daemon=True);pulse.start();time.sleep(.2)
    probe=next(a for a in json.load(urllib.request.urlopen(base+'/api/apps')) if a['name']=='XR Recovery Probe')
    post_json('/api/launch',{'id':probe['id'],'realm':'xr'})
    xid=wait(lambda:next((w['id'] for w in state()['windows'] if w['app_id']=='dome-xr-probe'),None))
    wait(lambda:any(e['kind']=='resize' for e in events('xr-only')))
    check(win(xid)['origin']=='xr' and not win(xid)['desktop_visible'],'XR launcher connects through XR-only Wayland socket')
    check(not color_at(350,300,(54,163,129)),'XR-only client is absent from desktop framebuffer')
    before=len(events('xr-only'));xr_packet([{'kind':'motion','id':xid,'uv':[.5,.5]}],[{'op':'focus','id':xid}]);key('x')
    wait(lambda:any(e['kind']=='key_press'and e.get('text')=='x'for e in events('xr-only')[before:]))
    check(True,'XR-only unmapped client retains live buffers and keyboard focus')
    key('Super_L',True);key('Return');key('Super_L',False);wait(lambda:win(xid)['maximized'])
    check(not win(xid)['desktop_visible'],'XR keyboard window shortcuts work without mapping the client to desktop')
    key('Super_L',True);key('Return');key('Super_L',False);wait(lambda:not win(xid)['maximized'])
    xr_shortcut(['Super_L','Alt_L'],'t');wait(lambda:not state()['presentation']['top'])
    check(state()['presentation']['bottom'],'Upper XR bar shortcut leaves taskbar visible')
    xr_shortcut(['Super_L','Alt_L'],'t');wait(lambda:state()['presentation']['top'])
    xr_shortcut(['Super_L','Alt_L'],'b');wait(lambda:not state()['presentation']['bottom'])
    check(state()['presentation']['top'],'Taskbar shortcut leaves upper XR bar visible')
    xr_shortcut(['Super_L','Alt_L'],'b');wait(lambda:state()['presentation']['bottom'])
    xr_shortcut(['Super_L','Shift_L'],'b');wait(lambda:not state()['presentation']['top'] and not state()['presentation']['bottom'])
    xr_shortcut(['Super_L','Shift_L'],'d');wait(lambda:not state()['presentation']['desktop'])
    before=len(events('xr-only'));key('z');xr_packet([{'kind':'motion','id':xid,'uv':[.5,.5]},{'kind':'button','button':0x110,'pressed':True},{'kind':'button','button':0x110,'pressed':False}]);time.sleep(.3)
    check(not any(e['kind']=='press' or (e['kind']=='key_press' and e.get('text')=='z') for e in events('xr-only')[before:]),'Hidden XR desktop rejects pointer clicks and ordinary typing')
    check(not win(xid)['desktop_visible'],'Intentionally hiding XR does not trigger desktop recovery')
    xr_shortcut(['Super_L','Shift_L'],'d');wait(lambda:state()['presentation']['desktop'])
    check(not state()['presentation']['top'] and not state()['presentation']['bottom'],'Restoring XR desktop preserves individual bar visibility')
    xr_shortcut(['Super_L','Shift_L'],'b');wait(lambda:state()['presentation']['top'] and state()['presentation']['bottom']);time.sleep(.3)
    layout=shell();xr_packet([{'kind':'motion','id':0,'uv':[20/layout['width'],18/layout['top']]},{'kind':'button','button':0x110,'pressed':True},{'kind':'button','button':0x110,'pressed':False}])
    wait(lambda:shell()['top']>36);layout=shell()
    xr_packet([{'kind':'motion','id':0,'uv':[100/layout['width'],166/layout['top']]},{'kind':'button','button':0x110,'pressed':True},{'kind':'button','button':0x110,'pressed':False}])
    wait(lambda:shell()['top']==shell()['height'])
    xr_shortcut(['Super_L','Shift_L'],'d');wait(lambda:not state()['presentation']['desktop'])
    xr_shortcut(['Super_L','Shift_L'],'d');wait(lambda:state()['presentation']['desktop'])
    check(shell()['top']==shell()['height'],'XR visibility shortcuts remain available from the appearance modal')
    key('Escape');wait(lambda:shell()['top']==36);time.sleep(.3)
    for linux in [0x110,0x112,0x111]:
        xr_packet([{'kind':'motion','id':xid,'uv':[.5,.5]},{'kind':'button','button':linux,'pressed':True}]);time.sleep(.04)
        motion(620+linux%10,400);wait(lambda:control()['mouse'])
        before=len(events('xr-only'));button();time.sleep(.85)
        check(control()['mouse'],'Held physical mouse button retains ownership past idle delay')
        xr_packet([{'kind':'reset'}]);time.sleep(.05)
        check(not any(e['kind']=='release'and e['button']==1 for e in events('xr-only')[before:]),'Hand reset cannot release physical mouse drag')
        button(False);wait(lambda:not control()['mouse']);time.sleep(.1)
    before=len(events('xr-only'));xr_packet([{'kind':'motion','id':xid,'uv':[.5,.5]},{'kind':'button','button':0x112,'pressed':True},{'kind':'button','button':0x112,'pressed':False}])
    wait(lambda:any(e['kind']=='press'and e['button']==4 for e in events('xr-only')[before:]))
    check(True,'Hand pointer resumes after mouse idle delay')
    heartbeat_stop.set();pulse.join(timeout=2)
    wait(lambda:win(xid)['desktop_visible'],timeout=5)
    check(win(xid)['origin']=='xr' and win(xid)['workspace']==0 and not win(xid)['minimized'],'Renderer heartbeat loss promotes the same XR client to primary desktop')
    check(wait(lambda:color_at(350,300,(54,163,129))),'Recovered XR client is stacked on primary output')
    action({'op':'close','id':xid});action({'op':'focus','id':bid})
    wait(lambda:len(state()['windows'])==2)
    if os.environ.get('DOME_TEST_XR')=='1':
        for panel in json.load(urllib.request.urlopen(base+'/api/state'))['panels']:
            action({'op':'dome_offset','id':panel['id'],'yaw_deg':-panel['yaw_deg']+(-22 if panel['id']==aid else 22),'pitch_deg':0})
            action({'op':'spatial_transform','id':panel['id'],'pose':None,'scale':0.4})
        xr_env={**env,'LD_LIBRARY_PATH':'/usr/lib/x86_64-linux-gnu','XDG_DATA_DIRS':'/usr/local/share:/usr/share','VK_DRIVER_FILES':'/usr/share/vulkan/icd.d/lvp_icd.json','VK_LOADER_DEBUG':'error,warn','SIMULATED_ENABLE':'1','XRT_COMPOSITOR_FORCE_XCB':'1','IPC_EXIT_WHEN_IDLE':'false','XR_RUNTIME_JSON':'/usr/share/openxr/1/openxr_monado.json'}
        # The installed service has CAP_SYS_NICE: secure execution ignores Vulkan ICD overrides.
        # A private unprivileged copy allows Lavapipe presentation on Xvfb.
        monado_binary=runtime/'monado-service';shutil.copyfile(shutil.which('monado-service'),monado_binary);monado_binary.chmod(0o700)
        monado=start([str(monado_binary)],'monado',xr_env,stdin=subprocess.PIPE)
        wait(lambda:(runtime/'monado_comp_ipc').exists())
        xr=start([str(root/'target/debug/dome-xr'),'--server',base,'--frames','540','--capture',str(out/'xr-textures'),'--report',str(out/'xr-report.json')],'native-xr',xr_env)
        def xr_window():
            for window in xroot.query_tree().children:
                if 'Monado' in str(window.get_wm_name()or''):return window
            return None
        mirror=wait(xr_window,timeout=20);time.sleep(1)
        geo=mirror.get_geometry();raw=mirror.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff);stereo=Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX');stereo.save(out/'01-native-xr-stereo.png')
        def mirror_image():
            raw=mirror.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
            return Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX')
        def bar_pixels(im):
            return sum(max(p)>15 for region in [(0,0,im.width,int(im.height*.22)),(0,int(im.height*.82),im.width,im.height)] for p in im.crop(region).getdata())
        host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
        original_bar_pixels=bar_pixels(stereo);desktop_bar=pixel(650,10)
        xr_shortcut(['Super_L','Shift_L'],'b');wait(lambda:not state()['presentation']['top']);time.sleep(.5)
        bars_hidden=mirror_image();bars_hidden.save(out/'01-native-xr-bars-hidden.png')
        check(bar_pixels(bars_hidden)<original_bar_pixels*.1,'Simulated headset slides both XR bars beyond the view edges')
        xr_shortcut(['Super_L','Shift_L'],'d');wait(lambda:not state()['presentation']['desktop']);time.sleep(.5)
        desktop_hidden=mirror_image();desktop_hidden.save(out/'01-native-xr-desktop-hidden.png')
        check(sum(max(p)>15 for p in desktop_hidden.getdata())<20,'Simulated headset fades the entire XR desktop out')
        check(pixel(650,10)==desktop_bar,'XR visibility shortcuts preserve desktop compositor bars')
        xr_shortcut(['Super_L','Shift_L'],'d');wait(lambda:state()['presentation']['desktop']);time.sleep(.5)
        check(bar_pixels(mirror_image())<original_bar_pixels*.1,'XR fade-in preserves hidden bar preferences')
        xr_shortcut(['Super_L','Shift_L'],'b');wait(lambda:state()['presentation']['top']);time.sleep(.5)
        restored=mirror_image();restored.save(out/'01-native-xr-restored.png')
        check(bar_pixels(restored)>original_bar_pixels*.8,'XR bars slide back into view after desktop fade-in')
        for id in [aid,bid]:action({'op':'pull_out','id':id,'head':{'position_m':[0,1.6,0],'orientation_xyzw':[0,0,0,1]}})
        time.sleep(.4);raw=mirror.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX').save(out/'01-native-xr-pulled.png')
        for id in [aid,bid]:action({'op':'return_to_dome','id':id})
        config=state()['config'];config['projection']='in_view';action({'op':'configure','config':config});time.sleep(.4)
        raw=mirror.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX').save(out/'01-native-xr-direct.png')
        check(xr.wait(timeout=45)==0,'Rust OpenXR frontend completes simulated session cleanly')
        for id in [aid,bid]:
            bordered=Image.open(out/'xr-textures'/f'window-{id}.png').convert('RGBA')
            check(bordered.getpixel((0,100))==(146,154,165,255) and bordered.getpixel((1,100))!=(146,154,165,255),'XR window has a one-pixel inset border',{'id':id})
        report=json.loads((out/'xr-report.json').read_text());check(report['peak_window_layers']==2 and report['peak_curved_layers']==2 and report['peak_flat_layers']==2 and report['peak_shell_layers']==2 and report['uploads']>1 and report['clean_exit'],'Monado presents spherical dome layers and flat pulled/direct-view layers',report)
        check(report['presentation_hidden_frames']>0 and report['presentation_fade_frames']>1 and report['presentation_slide_frames']>1,'OpenXR renders intermediate fade/slide frames and fully hidden frames',report)
        pixels=list(stereo.getdata());red_count=sum(r>100 and r>g*1.7 and r>b*1.4 for r,g,b in pixels);blue_count=sum(b>90 and b>r*1.8 and b>g*1.3 for r,g,b in pixels)
        check(red_count>200 and blue_count>200,'Simulated headset framebuffer contains both real client colors',{'red':red_count,'blue':blue_count})
        # Exercise the real renderer process, not only synthetic lease messages.
        wait(lambda:not control()['visible'])
        xr_crash=start([str(root/'target/debug/dome-xr'),'--server',base],'native-xr-recovery',xr_env)
        wait(lambda:control()['visible'],timeout=20)
        post_json('/api/launch',{'id':probe['id'],'realm':'xr'})
        recovery_id=wait(lambda:next((w['id'] for w in state()['windows'] if w['app_id']=='dome-xr-probe'),None))
        check(not win(recovery_id)['desktop_visible'],'Real OpenXR session keeps newly launched XR client off desktop')
        xr_crash.send_signal(signal.SIGSTOP)
        wait(lambda:win(recovery_id)['desktop_visible'],timeout=5)
        check(True,'Suspended XR renderer recovers its clients after heartbeat expiry')
        xr_crash.send_signal(signal.SIGCONT);wait(lambda:control()['visible'],timeout=10)
        check(win(recovery_id)['desktop_visible'],'Resuming XR leaves recovered desktop windows accessible')
        action({'op':'close','id':recovery_id});wait(lambda:len(state()['windows'])==2)
        post_json('/api/launch',{'id':probe['id'],'realm':'xr'})
        killed_id=wait(lambda:next((w['id'] for w in state()['windows'] if w['app_id']=='dome-xr-probe'),None))
        check(not win(killed_id)['desktop_visible'],'New XR launch after recovery is isolated again')
        xr_crash.kill();xr_crash.wait(timeout=5)
        wait(lambda:win(killed_id)['desktop_visible'],timeout=5)
        check(True,'SIGKILL of real OpenXR frontend promotes existing XR clients to primary desktop')
        action({'op':'close','id':killed_id});action({'op':'focus','id':bid});wait(lambda:len(state()['windows'])==2)
        monado.terminate();monado.wait(timeout=5)
        config=state()['config'];config['projection']='room_dome';action({'op':'configure','config':config})
        for id in [aid,bid]:
            action({'op':'dome_offset','id':id,'yaw_deg':0,'pitch_deg':0})
            action({'op':'spatial_transform','id':id,'pose':None,'scale':1.0})
        host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
    click(110,300);wait(lambda:state()['focused']==aid)
    check(wait(lambda:color_at(350,300,red)),'Click exposed window raises red surface over blue')
    count=len(events('a'));key('a');wait(lambda:any(e['kind']=='key_press'and e.get('text')=='a'for e in events('a')[count:]))
    check(True,'Keyboard input reaches the clicked Wayland client')
    key('Control_L',True);key('c');key('Control_L',False);wait(lambda:any(e['kind']=='clipboard_set'for e in events('a')))
    click(840,300);wait(lambda:state()['focused']==bid)
    before_a=len(events('a'));before_b=len(events('b'));motion(110,300);button(code=4);button(False,code=4)
    wait(lambda:any(e['kind']=='wheel'for e in events('a')[before_a:]))
    check(state()['focused']==bid and not any(e['kind']=='wheel'for e in events('b')[before_b:]),'Hover scrolling targets red while blue retains keyboard focus')
    key('b');wait(lambda:any(e['kind']=='key_press'and e.get('text')=='b'for e in events('b')[before_b:]))
    check(True,'Hover scrolling does not steal keyboard delivery')
    key('Control_L',True);key('v');key('Control_L',False)
    wait(lambda:any(e['kind']=='clipboard_get'and e['text']=='dome-a-clipboard'for e in events('b')))
    check(True,'Clipboard selection transfers between real Wayland clients')
    click(1100,600);time.sleep(.6)
    check(state()['focused']is None,'Clicking empty desktop clears shared and keyboard focus')
    count_a=len(events('a'));count_b=len(events('b'));key('c');time.sleep(.2)
    check(not any(e['kind']=='key_press'for e in events('a')[count_a:]+events('b')[count_b:]),'No client receives typing after desktop defocus')
    # Real client-initiated move/resize through xdg_toplevel requests and seat serials.
    click(110,300);wait(lambda:state()['focused']==aid);old=win(aid)['desktop'];drag(old['x']+80,old['y']+12,160,120)
    moved=win(aid)['desktop'];check(abs(moved['x']-old['x']-160)<4 and abs(moved['y']-old['y']-120)<4,'Client title drag updates native position and shared scene',moved)
    wait(lambda:any(e['kind']=='move_request'and e['accepted']for e in events('a')))
    drag(moved['x']+moved['w']-6,moved['y']+moved['h']-6,140,80)
    resized=win(aid)['desktop'];check(abs(resized['w']-860)<4 and abs(resized['h']-540)<4,'Client resize grab updates requested geometry',resized)
    wait(lambda:any(e['kind']=='resize'and abs(e['w']-860)<4 and abs(e['h']-540)<4 for e in events('a')))
    check(True,'Wayland client acknowledges and paints resized buffers');capture('02-moved-resized')
    background=pixel(1200,600);click(420,18);capture('02-classic-window-menu');click(430,54);wait(lambda:win(aid)['minimized'])
    check(pixel(1050,450)==background,'Native menu minimizes and removes the surface from output')
    click(160,770);wait(lambda:state()['focused']==aid and not win(aid)['minimized'])
    check(wait(lambda:color_at(1050,450,red)),'Native taskbar restores the minimized surface')
    click(420,18);click(430,82);wait(lambda:win(aid)['maximized'])
    wait(lambda:any(e['kind']=='resize'and e['w']==1280 and e['h']==704 for e in events('a')))
    check(wait(lambda:color_at(1200,600,red)),'Native maximize paints through the work area and reserves bars');capture('03-maximized')
    host.configure(width=1100,height=850);xd.sync()
    wait(lambda:any(e['kind']=='resize'and e['w']==1100 and e['h']==754 for e in events('a')))
    check(wait(lambda:color_at(1000,700,red)),'Output resize reconfigures maximized client and repaints new framebuffer');capture('03-output-resized')
    host.configure(width=1280,height=800);xd.sync()
    wait(lambda:host.get_geometry().width==1280 and host.get_geometry().height==800)
    wait(lambda:events('a')[-1]['kind']=='resize'and events('a')[-1]['w']==1280)
    click(420,18);click(430,82);wait(lambda:not win(aid)['maximized'])
    check(win(aid)['desktop']==resized,'Restore retains pre-maximize desktop geometry')
    # Native manager appears at the right of the taskbar; choose second desktop.
    click(1200,770);capture('04-desktop-manager');click(1000,470)
    wait(lambda:state()['active_workspace']==1)
    check(len(state()['windows'])==2 and state()['focused']is None,'Native desktop switch hides windows without closing clients')
    check(pixel(350,300)==background,'Inactive desktop surfaces are absent from the GPU output')
    click(1200,770);click(1000,438);wait(lambda:state()['active_workspace']==0)
    click(1200,770);click(1000,534);wait(lambda:len(state()['workspaces'])==4)
    check(True,'Native taskbar manager creates a virtual desktop')
    # Search an installed disposable desktop entry, with keys reserved from clients.
    before_a=len(events('a'));before_b=len(events('b'));click(45,770);text('fixture');capture('05-launcher');key('Return')
    wait(lambda:any(w['title']=='dome-launched'for w in state()['windows']))
    check(not any(e['kind']=='key_press'and e.get('text')in'fixture'for e in events('a')[before_a:]+events('b')[before_b:]),'Launcher search is reserved and launches a native Wayland client')
    check(not any(e['kind']=='key_release'and e.get('key')==16777220 for e in events('a')[before_a:]+events('b')[before_b:]),'Launcher confirmation does not leak an unmatched Return release')
    # Super+Space opens the same shell; Escape dismisses without activation.
    motion(1100,650);key('Super_L',True);key('space');key('Super_L',False);time.sleep(.2)
    check(wait(lambda:color_at(50,400,(26,35,45))),'Super+Space visibly opens native launcher')
    before_launched=len(events('launched'));capture('06-keyboard-launcher');key('Escape');time.sleep(.2)
    check(not color_at(50,400,(26,35,45)),'Escape visibly dismisses the native launcher')
    check(not any(e['kind'].startswith('key')and e.get('key')==16777216 for e in events('launched')[before_launched:]),'Launcher dismissal reserves both Escape press and release')
    count_a=len(events('a'));count_b=len(events('b'));key('z');time.sleep(.2)
    wait(lambda:any(e['kind']=='key_press'and e.get('text')=='z'for e in events('launched')[before_launched:]))
    check(not any(e['kind']=='key_press'and e.get('text')=='z'for e in events('a')[count_a:]+events('b')[count_b:]),'Launcher dismissal restores delivery to the launched client')
    def chord(modifiers,name):
        for modifier in modifiers:key(modifier,True)
        key(name)
        for modifier in reversed(modifiers):key(modifier,False)
        time.sleep(.2)
    launched=next(w['id']for w in state()['windows']if w['title']=='dome-launched')
    chord(['Super_L'],'Return');wait(lambda:win(launched)['maximized'])
    chord(['Super_L'],'Return');wait(lambda:not win(launched)['maximized'])
    check(True,'Super+Return toggles native maximize and restore')
    chord(['Alt_L'],'Tab');wait(lambda:state()['focused']!=launched);selected=state()['focused']
    check(selected in [aid,bid],'Alt+Tab cycles native window focus')
    chord(['Super_L'],'m');wait(lambda:win(selected)['minimized']);action({'op':'focus','id':selected})
    check(not win(selected)['minimized'],'Super+M minimizes and shared focus restores client')
    chord(['Super_L','Shift_L'],'2');wait(lambda:win(selected)['workspace']==1)
    chord(['Super_L'],'2');wait(lambda:state()['active_workspace']==1)
    chord(['Super_L','Shift_L'],'1');wait(lambda:win(selected)['workspace']==0)
    chord(['Super_L'],'1');wait(lambda:state()['active_workspace']==0)
    check(True,'Super+number switches desktops and Shift+Super+number moves clients')
    action({'op':'focus','id':aid});rect=win(aid)['desktop'];drag(rect['x']+100,rect['y']-15,24,-12)
    check(abs(win(aid)['desktop']['x']-rect['x']-24)<4,'Qt6 server title bar drags the native window')
    before=len(events('a'));chord(['Super_L'],'c');wait(lambda:any(e['kind']=='clipboard_set'for e in events('a')[before:]))
    copy_event=next(e for e in events('a')[before:]if e['kind']=='key_press'and e['key']==67)
    check(copy_event['modifiers']&0x04000000 and not copy_event['modifiers']&0x10000000,'Super+C translates to client Ctrl+C without a held Meta modifier')
    rect=win(aid)['desktop'];drag(rect['x']-2,rect['y']+80,-20,0);wait(lambda:abs(win(aid)['desktop']['w']-rect['w']-20)<4)
    check(abs(win(aid)['desktop']['x']-rect['x']+20)<4,'Qt6 left border resizes while retaining the opposite edge')
    rect=win(aid)['desktop'];click(rect['x']+rect['w']-62,rect['y']-15);wait(lambda:win(aid)['minimized'])
    check(True,'Qt6 native minimize button controls the real Wayland window')
    action({'op':'focus','id':aid})
    # Simulate an abrupt client crash; unrelated surfaces must survive.
    b.terminate();b.wait(timeout=3);wait(lambda:all(w['id']!=bid for w in state()['windows']))
    check(compositor.poll()is None and any(w['id']==aid for w in state()['windows']),'Client disconnect cleanup preserves other windows')
    for w in list(state()['windows']):action({'op':'close','id':w['id']})
    wait(lambda:not state()['windows'])
    check(compositor.poll()is None,'Close requests drain all client windows without compositor failure');capture('07-empty')
    # Application grouping, XDG actions and persisted launcher pins.
    for name,color in [('group-one','#36a381'),('group-two','#a36add')]:
        start([sys.executable,'scripts/desktop-client.py','--name',name,'--app-id','dome-fixture','--color',color,'--journal',str(out/(name+'.jsonl'))],name,client_env)
    wait(lambda:len(state()['windows'])==2 and {w['title'] for w in state()['windows']}=={'group-one','group-two'});order=[w['title']for w in state()['windows']];first_color=(54,163,129)if order[0]=='group-one'else(163,106,221);time.sleep(.3);motion(160,770)
    check(wait(lambda:color_at(170,600,first_color)),'Grouped task hover displays a live Wayland window thumbnail');capture('08-grouped-task-previews')
    click(290,600);wait(lambda:focused_title()==order[0])
    check(True,'Clicking grouped task preview activates its selected window')
    motion(160,770);button(code=4);wait(lambda:focused_title()==order[1]);button(False,code=4)
    check(True,'Wheel over grouped task cycles its windows')
    click(130,18);capture('09-xdg-application-menu');click(150,110)
    wait(lambda:any(w['title']=='dome-desktop-action'for w in state()['windows']))
    check(True,'Active application menu invokes an XDG Desktop Action through GIO')
    motion(160,770);button(code=3);button(False,code=3);click(200,686)
    pins=runtime/'config/dome/taskbar.json';check(wait(lambda:pins.exists()and json.loads(pins.read_text())==['dome-fixture.desktop']),'Task context menu persists a pin by XDG desktop-file ID')
    motion(160,770);button(code=3);button(False,code=3);click(200,718);wait(lambda:not state()['windows'])
    check(True,'Grouped task Close All closes every application window')
    click(160,770);wait(lambda:len(state()['windows'])==1)
    check(True,'Pinned task launches the application after all its windows close')
    for w in list(state()['windows']):action({'op':'close','id':w['id']})
    wait(lambda:not state()['windows'])
    errors=[s for s in(out/'compositor.log').read_text().splitlines()if 'panicked at'in s or 'ERROR'in s]
    check(not errors,'Rust compositor emits no runtime errors',errors)
    result=0
except Exception as e:
    print('DESKTOP TEST FAILED:',repr(e),flush=True)
    try:(out/'failure-state.json').write_text(json.dumps(state(),indent=2))
    except Exception:pass
    if not checks or checks[-1]['passed']:checks.append({'passed':False,'check':str(e)})
    if xd is not None:
        try:capture('failure')
        except Exception:pass
finally:
    if xd is not None:xd.close()
    for p in reversed(processes):
        if p.poll()is None:p.terminate()
    for p in reversed(processes):
        try:p.wait(timeout=3)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    for log in logs:log.close()
    (out/'report.json').write_text(json.dumps({'checks':checks,'failed':sum(not c['passed']for c in checks),'renderer':'Rust/Smithay GLES on isolated Xvfb/Mesa','clients':'Native Wayland Qt fixtures; no Godot'},indent=2)+'\n')
raise SystemExit(result)
