#!/usr/bin/env python3
"""Real portal + PipeWire + native consent tests on a private bus, display and graph.
Run: uv run --no-project --with dbus-next --with python-xlib --with pillow --with PySide6 python scripts/test-screencast.py
"""
import asyncio,json,os,re,signal,shutil,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK, [])
from dbus_next.aio import MessageBus
from dbus_next import Message,MessageType,Variant
from Xlib import X,XK,display
from Xlib.ext import xtest
import PySide6

if os.environ.get('DOME_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/screencast';out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[];checks=[]
def check(value,name):
    checks.append({'passed':bool(value),'check':name});print(('PASS 'if value else'FAIL ')+name,flush=True)
    assert value,name

def start(args,name,env,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log)
    p=subprocess.Popen(args,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,**kw);processes.append(p);return p
async def wait(fn,timeout=12):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError):pass
        await asyncio.sleep(.05)
    raise AssertionError('Timed out: '+str(fn))

async def main(directory):
    runtime=Path(directory);runtime.chmod(0o700)
    output_x=int(os.environ.get('DOME_TEST_OUTPUT_X','0'))
    for d in ['data/xdg-desktop-portal/portals','config/xdg-desktop-portal','empty','home']:(runtime/d).mkdir(parents=True,exist_ok=True)
    (runtime/'config/xdg-desktop-portal/dome-portals.conf').write_text('[preferred]\ndefault=none\norg.freedesktop.impl.portal.ScreenCast=dome-screencast\n')
    shutil.copy(root/'packaging/dome-screencast.portal',runtime/'data/xdg-desktop-portal/portals/dome-screencast.portal')
    defaults=os.environ.get('DOME_TEST_PORTAL_DEFAULTS')=='1'
    if defaults:
        shutil.copy(root/'packaging/dome-portals.conf',runtime/'config/xdg-desktop-portal/dome-portals.conf')
        shutil.copy(root/'packaging/dome-settings.portal',runtime/'data/xdg-desktop-portal/portals/dome-settings.portal')
    # No hardware or host policy manager is touched.
    config=runtime/'pipewire.conf';config.write_text('''context.properties = { core.daemon = true core.name = pipewire-0 }
context.spa-libs = { support.* = support/libspa-support }
context.modules = [
 { name = libpipewire-module-protocol-native }
 { name = libpipewire-module-client-node }
 { name = libpipewire-module-metadata }
 { name = libpipewire-module-link-factory }
 { name = libpipewire-module-access }
]
''')
    rd,wr=os.pipe();start([os.environ.get('DOME_XVFB')or shutil.which('Xvfb')or str(root/'.local-deps/root/usr/bin/Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as pipe:display_name=':'+pipe.readline().strip()
    with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    env={**os.environ,'DISPLAY':display_name,'XDG_RUNTIME_DIR':directory,'PIPEWIRE_RUNTIME_DIR':directory,
         'HOME':str(runtime/'home'),'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_DATA_HOME':str(runtime/'data'),
         'XDG_DATA_DIRS':str(runtime/'empty'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_CURRENT_DESKTOP':'Dome',
         'XDG_SESSION_TYPE':'wayland','DOME_LOGIN_SESSION':'0','DOME_SCREENCAST':'1','DOME_START_XR':'0',
         'GSETTINGS_SCHEMA_DIR':'/usr/share/glib-2.0/schemas','DOME_APPEARANCE_SERVICE':'1'if defaults else'0','DOME_RENDERER':os.environ.get('DOME_TEST_RENDERER','gles'),
         'DOME_PREVIEW_PORT':str(port),'LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn',
         'DBUS_SYSTEM_BUS_ADDRESS':os.environ['DBUS_SESSION_BUS_ADDRESS']}
    if output_x:
        geometry=runtime/'geometry.json'
        geometry.write_text(json.dumps({'displays':[{'name':'winit','x':output_x}]}))
        env['DOME_CONFIG']=str(geometry)
    for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','QT_PLUGIN_PATH','QT_QPA_PLATFORM_PLUGIN_PATH','GTK_MODULES']:env.pop(name,None)
    if env['DOME_RENDERER']=='vulkan':
        env.pop('LIBGL_ALWAYS_SOFTWARE',None)
        env['XDG_DATA_DIRS']+=':/usr/share'  # Vulkan ICD manifests live here.
    pw=start(['pipewire','-c',str(config)],'pipewire',env)
    await wait(lambda:(runtime/'pipewire-0').is_socket())
    policy=start(['wireplumber','--profile','policy'],'wireplumber',env)
    compositor=start([os.environ.get('DOME_COMPOSITOR',str(root/'target/debug/dome-compositor')),'--backend','nested'],'compositor',env)
    log=await wait(lambda: (out/'compositor.log').read_text() if 'Dome Wayland socket:' in (out/'compositor.log').read_text() else None)
    env['WAYLAND_DISPLAY']=re.search(r'Dome Wayland socket: (\S+)',log)[1]
    base=f'http://127.0.0.1:{port}'
    def state():return json.load(urllib.request.urlopen(base+'/api/state',timeout=2))['scene']
    def action(data):
        urllib.request.urlopen(urllib.request.Request(base+'/api/action',data=json.dumps(data).encode(),headers={'Content-Type':'application/json'}),timeout=2).close()
    xd=display.Display(display_name)
    host=await wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
    host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
    def key(name):
        code=xd.keysym_to_keycode(XK.string_to_keysym(name))
        for event in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,event,code)
        xd.sync()
    def click(x,y):
        geometry=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(x-output_x+geometry.x),y=int(y+geometry.y))
        xd.sync();time.sleep(.08)
        for event in [X.ButtonPress,X.ButtonRelease]:
            xtest.fake_input(xd,event,1);xd.sync();time.sleep(.08)
    picker=lambda:next((w for w in state()['windows']if w['title'].startswith('Share your screen')),None)
    async def consent(accept=True):
        w=await wait(picker);action({'op':'focus','id':w['id']});await asyncio.sleep(.3)
        if accept:
            fltk=os.environ.get('DOME_TEST_PICKER_FRONTEND','fltk')=='fltk'
            r=w['desktop'];click(r['x']+90,r['y']+(100 if fltk else 74 if defaults else 60));await asyncio.sleep(.1)
            from PIL import Image
            g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
            Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/'consent.png')
            if fltk:
                click(r['x']+515,r['y']+326)
            else:click(r['x']+r['w']-(55 if defaults else 140),r['y']+r['h']-25)
        else:key('Escape')
    # Start the real frontend directly: this private D-Bus has no systemd manager.
    subprocess.run(['dbus-update-activation-environment','HOME','DISPLAY','WAYLAND_DISPLAY','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_DATA_DIRS','XDG_STATE_HOME','XDG_CURRENT_DESKTOP','GSETTINGS_SCHEMA_DIR'],env=env,check=True)
    if defaults:
        # Match the installed Qt6 portal service drop-in, without touching systemd
        # or the user's portal. D-Bus activation on this private bus has no manager.
        start(['/usr/libexec/xdg-desktop-portal-kde'],'qt6-portal',
              {**env,'QT_QPA_PLATFORM':'wayland','QT_QPA_PLATFORMTHEME':'kde'})
    portal=start(['/usr/libexec/xdg-desktop-portal','--verbose'],'portal',env)
    bus=await MessageBus(negotiate_unix_fd=True).connect();responses={}
    def incoming(message):
        if message.message_type==MessageType.SIGNAL and message.interface=='org.freedesktop.portal.Request' and message.member=='Response':
            responses[message.path]=message.body
    bus.add_message_handler(incoming)
    match=await bus.call(Message(destination='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='AddMatch',signature='s',body=["type='signal',interface='org.freedesktop.portal.Request'"]))
    request_serial=0
    async def call(member,signature,body,interface='org.freedesktop.portal.ScreenCast',path='/org/freedesktop/portal/desktop',destination='org.freedesktop.portal.Desktop'):
        nonlocal request_serial
        if interface in ['org.freedesktop.portal.ScreenCast','org.freedesktop.portal.FileChooser'] and body and isinstance(body[-1],dict):
            request_serial+=1
            body=[*body[:-1],{**body[-1],'handle_token':Variant('s','dome_request_'+str(request_serial))}]
        reply=await asyncio.wait_for(bus.call(Message(destination=destination,path=path,interface=interface,member=member,signature=signature,body=body)),15)
        assert reply.message_type!=MessageType.ERROR,(member,reply.error_name,reply.body)
        return reply.body
    await wait(lambda:'org.freedesktop.portal.Desktop' in subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.ListNames'],capture_output=True,text=True).stdout)
    props=(await call('GetAll','s',['org.freedesktop.portal.ScreenCast'],'org.freedesktop.DBus.Properties'))[0]
    check(props['AvailableSourceTypes'].value==1 and props['AvailableCursorModes'].value==3,'Real frontend advertises DomeD monitors and cursor modes')
    if defaults:
        # Use the public frontend, then verify its chosen general-dialog backend
        # is actually linked to Qt6 rather than assuming the config was honored.
        chooser=(await call('OpenFile','ssa{sv}',['','Qt6 portal check',{}],'org.freedesktop.portal.FileChooser'))[0]
        await wait(lambda:any(w['title']=='Qt6 portal check' for w in state()['windows']))
        pid=(await call('GetConnectionUnixProcessID','s',['org.freedesktop.impl.portal.desktop.kde'],
            'org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus'))[0]
        check('libQt6Widgets' in Path(f'/proc/{pid}/maps').read_text(),'General file dialogs default to the real Qt6 KDE portal')
        await call('Close','',[],'org.freedesktop.portal.Request',chooser)
    # Fixture supplies distinctive non-black pixels that can be checked after capture.
    fixture_env={**env,'QT_QPA_PLATFORM':'wayland','LD_LIBRARY_PATH':str(Path(PySide6.__file__).parent/'Qt/lib')}
    start([sys.executable,'scripts/desktop-client.py','--name','ScreenCast pixels','--color','#bd3e4e','--journal',str(out/'fixture.jsonl')],'fixture',fixture_env)
    await wait(lambda:any(w['title']=='ScreenCast pixels' for w in state()['windows']))
    async def request(member,signature,body):
        path=(await call(member,signature,body))[0]
        await wait(lambda:path in responses,20)
        return responses.pop(path)
    serial=0
    async def session(cursor=1):
        nonlocal serial
        serial+=1
        code,data=await request('CreateSession','a{sv}',[{'session_handle_token':Variant('s','dome_test_'+str(serial))}]);assert code==0,(code,data)
        path=data['session_handle'].value
        code,data=await request('SelectSources','oa{sv}',[path,{'types':Variant('u',1),'multiple':Variant('b',True),'cursor_mode':Variant('u',cursor)}]);assert code==0
        return path
    path=await session()
    task=asyncio.create_task(request('Start','osa{sv}',[path,'',{}]));await consent(False)
    check((await task)[0]==1,'Native picker cancel grants no capture')
    path=await session()
    task=asyncio.create_task(request('Start','osa{sv}',[path,'',{}]));await consent()
    code,data=await task;check(code==0,'Native monitor selection starts a real PipeWire stream')
    streams=data['streams'].value;check(len(streams)==1,'Start returns the selected monitor only')
    node,props=streams[0];check(props['position'].value==[output_x,0] and props['size'].value==[1280,800],'Stream reports logical monitor position and dimensions')
    # Consumer connects to the actual PipeWire node through isolated policy.
    frame=out/'frames.rgba';frame.unlink(missing_ok=True)
    remote=await bus.call(Message(destination='org.freedesktop.portal.Desktop',path='/org/freedesktop/portal/desktop',
        interface='org.freedesktop.portal.ScreenCast',member='OpenPipeWireRemote',signature='oa{sv}',body=[path,{}]))
    assert remote.message_type!=MessageType.ERROR,(remote.error_name,remote.body)
    fd=remote.unix_fds[remote.body[0]]
    consumer=start(['gst-launch-1.0','-q','pipewiresrc',f'fd={fd}',f'path={node}','always-copy=true','num-buffers=3','!','video/x-raw,format=RGBA','!','filesink',f'location={frame}'],'consumer',env,pass_fds=(fd,))
    os.close(fd)
    await asyncio.sleep(2)
    (out/'graph.json').write_text(subprocess.run(['pw-dump'],env=env,capture_output=True,text=True,timeout=3).stdout)
    await wait(lambda:consumer.poll()is not None,18)
    check(consumer.returncode==0 and frame.stat().st_size==1280*800*4*3,'Portal-authorized PipeWire remote delivers three complete RGBA frames')
    pixels=frame.read_bytes();check(pixels.count(bytes.fromhex('bd3e4eff'))>10000,'Captured frames contain the composed Wayland client pixels')
    from PIL import Image
    captured=Image.frombytes('RGBA',(1280,800),pixels[-1280*800*4:]);captured.save(out/'captured.png')
    check(captured.getpixel((110,100))[:3]==(189,62,78) and captured.getpixel((110,700))[:3]!=(189,62,78),'Captured image has correct orientation and RGBA channels')
    await call('Close','',[],'org.freedesktop.portal.Session',path)
    def node_gone():
        objects=json.loads(subprocess.run(['pw-dump'],env=env,capture_output=True,text=True,timeout=3).stdout)
        return not any(o['id']==node and o['type']=='PipeWire:Interface:Node' and o.get('info',{}).get('props',{}).get('node.name','').startswith('DomeD ') for o in objects)
    check(await wait(node_gone),'Closing the portal session destroys its PipeWire node')
    # Backend calls from arbitrary bus peers cannot bypass the frontend.
    reply=await call('CreateSession','oosa{sv}',['/test/request','/test/session','test',{}],
        'org.freedesktop.impl.portal.ScreenCast',destination='org.freedesktop.impl.portal.desktop.dome.screencast')
    check(reply[0]==2,'Direct backend access is rejected')
    # Close an in-progress request while its native consent dialog is open.
    path=await session(2)
    pending=(await call('Start','osa{sv}',[path,'',{}]))[0];await wait(picker)
    await call('Close','',[],'org.freedesktop.portal.Request',pending)
    await wait(lambda:not picker())
    check(True,'Request.Close cancels the native dialog without granting capture')
    # Embedded mode uses a tray indicator instead of an always-open window.
    path=await session(2)
    task=asyncio.create_task(request('Start','osa{sv}',[path,'',{}]));await consent()
    code,data=await task;check(code==0,'Embedded cursor capture starts successfully')
    node=data['streams'].value[0][0]
    check(not any(w['title'].startswith('Screen sharing active') for w in state()['windows']),
          'Screen sharing creates no permanent status window')
    entries=(await call('Get','ss',['org.kde.StatusNotifierWatcher','RegisteredStatusNotifierItems'],
        'org.freedesktop.DBus.Properties','/StatusNotifierWatcher','org.kde.StatusNotifierWatcher'))[0].value
    indicator=None
    for entry in entries:
        service,_,path=entry.partition('/')
        reply=await bus.call(Message(destination=service,path='/'+path,interface='org.freedesktop.DBus.Properties',
            member='Get',signature='ss',body=['org.kde.StatusNotifierItem','Id']))
        if reply.message_type!=MessageType.ERROR and reply.body[0].value=='dome.screen-sharing':
            indicator=(service,'/'+path);break
    check(indicator is not None,'Active capture publishes a StatusNotifier tray indicator')
    service,path_item=indicator
    layout=await call('GetLayout','iias',[0,-1,[]],
        'com.canonical.dbusmenu',path_item+'/Menu',service)
    check(layout[1][2][0].value[1]['label'].value=='Stop sharing',
          'Tray menu exposes Stop sharing')
    await call('Event','isvu',[1,'clicked',Variant('i',0),0],
        'com.canonical.dbusmenu',path_item+'/Menu',service)
    check(await wait(node_gone),'Tray Stop sharing destroys the stream')
    path=await session()
    task=asyncio.create_task(request('Start','osa{sv}',[path,'',{}]));await consent()
    code,data=await task;assert code==0
    node=data['streams'].value[0][0]
    frontend_pid=(await call('GetConnectionUnixProcessID','s',['org.freedesktop.portal.Desktop'],
        'org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus'))[0]
    os.kill(frontend_pid,signal.SIGTERM)
    check(await wait(node_gone),'Frontend exit revokes capture and closes the sharing control')
    bus.disconnect();xd.close()

def cleanup():
    for p in reversed(processes):
        if p.poll()is None:p.terminate()
        try:p.wait(timeout=4)
        except subprocess.TimeoutExpired:p.kill();p.wait()
class RuntimeDirectory(tempfile.TemporaryDirectory):
    def __exit__(self,*args):
        cleanup()
        subprocess.run(['fusermount3','-u',str(Path(self.name)/'doc')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        return super().__exit__(*args)
result=1
try:
    with RuntimeDirectory(prefix='dome-screencast-')as directory:asyncio.run(main(directory))
    result=0
finally:
    cleanup()
    for log in logs:log.close()
    (out/'report.json').write_text(json.dumps({'passed':result==0,'checks':checks},indent=2)+'\n')
sys.exit(result)
