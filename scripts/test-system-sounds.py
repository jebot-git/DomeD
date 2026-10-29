#!/usr/bin/env python3
"""Private DomeD notification, Wayland bell and native settings sound acceptance.

The libcanberra fixture records playback instead of connecting to desktop audio.
All displays, buses, profiles and compositor instances belong to this test.
"""
import argparse,asyncio,contextlib,hashlib,json,os,re,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from PIL import Image,ImageChops
from Xlib import display,X,XK
from Xlib.ext import xtest
from dbus_next import Variant
from dbus_next.aio import MessageBus
if os.environ.get('DOME_SOUND_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_SOUND_TEST_BUS':'1'})
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
out=a.output.resolve();out.mkdir(exist_ok=False);root=Path(__file__).resolve().parents[1];checks=[];children=[];logs=[];compositor=None;failure=None

def check(value,name):
    checks.append({'name':name,'passed':bool(value)});print(('PASS 'if value else'FAIL ')+name,flush=True);assert value,name

def wait(fn,seconds=15):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        if compositor:assert compositor.poll()is None,'Private compositor exited'
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError):pass
        time.sleep(.03)
    raise AssertionError('Timed out; inspect '+str(out))

def start(args,name,env,**kwargs):
    log=(out/(name+'.log')).open('w');logs.append(log);child=subprocess.Popen(list(map(str,args)),env=env,stdout=log,stderr=log,**kwargs);children.append(child);return child

def stop(child):
    if child.poll()is None:
        child.terminate()
        try:child.wait(timeout=8)
        except subprocess.TimeoutExpired:child.kill();child.wait()

async def send(hints=None,replace=0,capabilities=False,close=0):
    bus=await MessageBus().connect()
    try:
        obj=await bus.introspect('org.freedesktop.Notifications','/org/freedesktop/Notifications')
        interface=bus.get_proxy_object('org.freedesktop.Notifications','/org/freedesktop/Notifications',obj).get_interface('org.freedesktop.Notifications')
        if capabilities:return await interface.call_get_capabilities()
        if close:return await interface.call_close_notification(close)
        return await interface.call_notify('Sound fixture',replace,'','Private test','Local notification',[],hints or {},15000)
    finally:bus.disconnect()

try:
    with tempfile.TemporaryDirectory(prefix='dome-system-sounds-')as temporary:
        runtime=Path(temporary);runtime.chmod(0o700);shim=runtime/'shim';shim.mkdir()
        subprocess.run(['cc','-shared','-fPIC','-O2',str(root/'scripts/system-sound-fixture.c'),'-o',str(shim/'libcanberra.so.0')],check=True)
        xml=Path('/usr/share/wayland-protocols/staging/xdg-system-bell/xdg-system-bell-v1.xml')
        for mode,name in [('client-header','xdg-system-bell-v1-client-protocol.h'),('private-code','bell-protocol.c')]:subprocess.run(['wayland-scanner',mode,str(xml),str(runtime/name)],check=True)
        flags=subprocess.check_output(['pkg-config','--cflags','--libs','wayland-client'],text=True).split()
        subprocess.run(['cc','-O2','-I'+str(runtime),str(root/'scripts/system-bell-client.c'),str(runtime/'bell-protocol.c'),'-o',str(runtime/'bell-client'),*flags],check=True)
        with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        base=f'http://127.0.0.1:{port}';record=out/'sound-events.txt'
        env={k:v for k,v in os.environ.items()if not k.startswith('DOME_')}
        env.update(HOME=temporary,XDG_RUNTIME_DIR=temporary,XDG_CONFIG_HOME=temporary+'/config',XDG_DATA_HOME=temporary+'/data',XDG_STATE_HOME=temporary+'/state',XDG_CACHE_HOME=temporary+'/cache',XDG_DATA_DIRS=temporary+'/empty',PIPEWIRE_RUNTIME_DIR=temporary,PIPEWIRE_REMOTE='no-host-audio',PULSE_SERVER='unix:'+temporary+'/no-host-audio',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+temporary+'/no-system-bus',DOME_LOGIN_SESSION='0',DOME_SESSION_RESTORE='0',DOME_START_XR='0',DOME_RENDERER='gles',DOME_APPEARANCE_SERVICE='0',LIBGL_ALWAYS_SOFTWARE='1',DOME_PREVIEW_PORT=str(port),DOME_SOUND_TEST_RECORD=str(record),LD_LIBRARY_PATH=str(shim)+':'+os.environ.get('LD_LIBRARY_PATH',''),RUST_LOG='warn')
        for name in ['TRAY','KDECONNECT','KWALLET','POLKIT','IME','XWAYLAND']:env['DOME_START_'+name]='0'
        for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(name,None)
        rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',env,pass_fds=(wr,));os.close(wr);env['DISPLAY']=':'+os.read(rd,64).decode().strip();os.close(rd)
        connection=display.Display(env['DISPLAY'])
        def get(path):
            with urllib.request.urlopen(base+path,timeout=2)as response:return json.load(response)
        def boot(name,fail=False):
            global compositor
            current=env.copy()
            if fail:current['DOME_SOUND_FIXTURE_FAIL']='1'
            compositor=start([a.binary.resolve(),'--backend','nested'],name,current)
            wait(lambda:get('/api/state')['scene']['source']=='wayland')
            host=wait(lambda:next((w for w in connection.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
            host.set_input_focus(X.RevertToParent,X.CurrentTime);connection.sync();time.sleep(.4)
            wait(lambda:'Dome Wayland socket:'in(out/(name+'.log')).read_text())
            wayland=re.search(r'Dome Wayland socket: (\S+)',(out/(name+'.log')).read_text())[1]
            return host,{**env,'WAYLAND_DISPLAY':wayland}
        def plays():return [line[5:]for line in record.read_text().splitlines()if line.startswith('play ')]if record.exists()else[]
        def notice(hints=None,replace=0):time.sleep(.2);return asyncio.run(send(hints,replace))
        def audible(hints,expected):
            count=len(plays());identifier=notice(hints);wait(lambda:len(plays())>count);check(plays()[-1]==expected,'Notification uses '+expected);return identifier
        def key(name,modifiers=()):
            for item in (*modifiers,name):xtest.fake_input(connection,X.KeyPress,connection.keysym_to_keycode(XK.string_to_keysym(item)))
            for item in (name,*reversed(modifiers)):xtest.fake_input(connection,X.KeyRelease,connection.keysym_to_keycode(XK.string_to_keysym(item)))
            connection.sync();time.sleep(.04)
        def capture(name):
            g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);image=Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX');image.save(out/(name+'.png'));return image
        settings=runtime/'config/dome/sounds.json'
        def enabled():return json.loads(settings.read_text())['enabled']if settings.exists()else True
        def open_settings():
            key('comma',('Super_L',));wait(lambda:get('/api/xr-shell')['top']>36);key('End');time.sleep(.2)
        def toggle(expected):
            key('Return');wait(lambda:enabled()==expected);time.sleep(.2)
        def xr_toggle(expected):
            layout=get('/api/xr-shell');width=layout['width'];height=layout['height']
            w=min(800,width-16);h=min(700,height-100);x=(width-w)/2+w*.75;y=36+max(0,(height-96-h)/2)+40+4.5*34
            panel=layout.get('pullout')is not None
            first,rows=(36,min(layout['top'],height-60)-36)if panel else(0,min(layout['top'],36))
            events=[{'kind':'motion','id':2**64-6 if panel else 0,'uv':[x/width,(y-first)/rows]}, {'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}]
            request=urllib.request.Request(base+'/api/xr-input',data=json.dumps({'events':events,'actions':[]}).encode(),headers={'Content-Type':'application/json'})
            with urllib.request.urlopen(request,timeout=2)as response:assert response.status==200
            wait(lambda:enabled()==expected);time.sleep(.6)
        host,client_env=boot('compositor')
        check('sound'in asyncio.run(send(capabilities=True)),'Notification server advertises sound support')
        identifier=audible({},'message-new-instant')
        audible({'urgency':Variant('y',2)},'dialog-warning')
        audible({'sound-name':Variant('s','complete')},'complete')
        count=len(plays());notice({'suppress-sound':Variant('b',True),'sound-name':Variant('s','complete')});time.sleep(.3);check(len(plays())==count,'Client suppression takes precedence over a named sound')
        count=len(plays());notice({},identifier);time.sleep(.3);check(len(plays())==count,'Replacing a displayed notification does not replay its sound')
        for ident in range(1,5):asyncio.run(send(close=ident))
        open_settings();before=capture('settings-on');toggle(False);after=capture('settings-off')
        g=host.get_geometry();w=min(800,g.width-16);h=min(700,g.height-100);x=(g.width-w)//2;y=36+max(0,(g.height-96-h)//2);region=(x+w*2//3,y+40+4*34,x+w-20,y+40+5*34)
        check(ImageChops.difference(before.crop(region),after.crop(region)).getbbox()is not None,'Native settings visibly change On to Off')
        check(settings.stat().st_mode&0o777==0o600,'Sound preference is saved privately')
        count=len(plays());notice();time.sleep(.3);check(len(plays())==count,'Session mute suppresses notification playback')
        key('Escape');subprocess.run([str(runtime/'bell-client')],env=client_env,check=True);time.sleep(.3);check(len(plays())==count,'Session mute also suppresses native Wayland bells')
        open_settings();xr_toggle(True);check(enabled(),'Shared XR settings pointer can reenable session sounds');key('Escape');time.sleep(.2)
        count=len(plays());subprocess.run([str(runtime/'bell-client')],env=client_env,check=True);wait(lambda:len(plays())>count);check(plays()[-1]=='bell-window-system','Native Wayland bell reaches event sound playback')
        open_settings();toggle(False);key('Escape');stop(compositor);compositor=None
        host,client_env=boot('restart-muted');check(not enabled(),'Disabled sounds persist across compositor restart')
        count=len(plays());notice();time.sleep(.3);check(len(plays())==count,'Restart never replays muted notifications')
        open_settings();toggle(True);key('Escape');audible({},'message-new-instant')
        stop(compositor);compositor=None
        host,client_env=boot('backend-unavailable',fail=True);notice();time.sleep(.2)
        check(compositor.poll()is None and get('/api/state')['scene']['source']=='wayland','Missing audio backend leaves notification and desktop service usable')
        stop(compositor);compositor=None;connection.close()
except BaseException as ex:
    failure=repr(ex);raise
finally:
    for child in reversed(children):stop(child)
    for log in logs:log.close()
    (out/'report.json').write_text(json.dumps({'passed':failure is None and bool(checks),'failure':failure,'checks':checks,'binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'test_source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'scope':'Private compositor/bus/display, native settings and Wayland protocol; libcanberra test double prevents any host audio playback','desktop_sessions_restarted':False},indent=2)+'\n')
