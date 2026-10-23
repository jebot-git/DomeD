#!/usr/bin/env python3
"""Real PipeWire tray tests on private PipeWire, policy-only WirePlumber, D-Bus and Xvfb.
Never connects to the host audio graph or starts hardware discovery.
Run: uv run --no-project --with python-xlib --with pillow python scripts/test-audio.py
"""
import json,os,re,socket,subprocess,sys,tempfile,time,urllib.request,shutil,signal
signal.pthread_sigmask(signal.SIG_SETMASK, [])
from pathlib import Path
from Xlib import X,XK,display
from Xlib.ext import xtest
from PIL import Image
if os.environ.get('DOME_AUDIO_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_AUDIO_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/audio';out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[];checks=[]
def start(args,name,env,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log);p=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT,**kw);processes.append(p);return p

def wait(fn,timeout=40):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/audio')

def check(ok,label):
    checks.append({'check':label,'passed':bool(ok)});print(('PASS 'if ok else'FAIL ')+label,flush=True);assert ok,label

def main(directory):
    with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    base=f'http://127.0.0.1:{port}'
    env={**os.environ,'XDG_RUNTIME_DIR':directory,'PIPEWIRE_RUNTIME_DIR':directory,'PIPEWIRE_REMOTE':'pipewire-0','PULSE_SERVER':'unix:'+directory+'/pulse/native','HOME':directory,'XDG_CONFIG_HOME':directory+'/config','XDG_STATE_HOME':directory+'/state','XDG_DATA_HOME':directory+'/data','XDG_DATA_DIRS':directory+'/empty','XDG_CURRENT_DESKTOP':'Dome','DOME_BACKEND':'nested','DOME_RENDERER':'gles','DOME_START_XR':'0','DOME_START_IME':'0','DOME_SESSION_RESTORE':'0','DOME_LOGIN_SESSION':'0','LIBGL_ALWAYS_SOFTWARE':'1','DOME_PREVIEW_PORT':str(port),'RUST_LOG':'warn','DBUS_SYSTEM_BUS_ADDRESS':os.environ['DBUS_SESSION_BUS_ADDRESS']}
    for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','PAM_KWALLET5_LOGIN']:env.pop(key,None)
    Path(directory+'/config/dome').mkdir(parents=True);Path(directory+'/config/dome/keyboard.json').write_text('{"ime":false}')
    Path(directory+'/config/dome/appearance.json').write_text(json.dumps({'font':'Unifont / Sans-Serif','font_size':16,'font_scale':1,'accent':'#6babff','light':False}))
    def command(*args):
        p=subprocess.run(args,env=env,capture_output=True,timeout=4)
        if p.returncode:
            (out/'last-command-error.txt').write_text(repr(args)+'\n'+p.stderr.decode(errors='replace'))
            p.check_returncode()
        return p.stdout
    def graph():return json.loads(command('pw-dump'))
    def nodes():return {o['info']['props']['node.name']:o for o in graph() if o.get('type')=='PipeWire:Interface:Node'}
    def defaults():
        return {m['key']:m['value']['name'] for o in graph() if o.get('type')=='PipeWire:Interface:Metadata' and o.get('props',{}).get('metadata.name')=='default' for m in o.get('metadata',[]) if m['key']in ['default.audio.sink','default.audio.source']}
    def level(target='@DEFAULT_AUDIO_SINK@'):
        text=command('wpctl','get-volume',target).decode();match=re.search(r'Volume: ([0-9.]+)',text);return (float(match[1]),'[MUTED]'in text)if match else None
    def seed():
        def set_defaults():
            available=nodes()
            if len(available)!=4:return False
            for name in ['dome.test.speakers','dome.test.mic']:command('wpctl','set-default',str(available[name]['id']))
            return defaults().get('default.audio.sink')=='dome.test.speakers' and defaults().get('default.audio.source')=='dome.test.mic'
        wait(set_defaults)
        command('wpctl','set-volume','@DEFAULT_AUDIO_SINK@','0.40');command('wpctl','set-volume','@DEFAULT_AUDIO_SOURCE@','0.60')
    pipewire=start(['pipewire','-c',str(root/'scripts/audio-test.conf')],'pipewire',env);wait(lambda:Path(directory+'/pipewire-0').is_socket())
    policy=start(['wireplumber','--profile','policy'],'wireplumber',env);seed()
    check(all(name.startswith('dome.test.')for name in nodes()),'Audio fixture contains only isolated virtual devices')
    rd,wr=os.pipe();start([os.environ.get('DOME_AUDIO_XVFB',shutil.which('Xvfb') or str(root/'.local-deps/root/usr/bin/Xvfb')),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp','-noreset'],'xvfb',env,pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as f:env['DISPLAY']=':'+f.readline().strip()
    env['XDG_SESSION_TYPE']='x11'
    compositor=start([os.environ.get('DOME_COMPOSITOR_BIN',str(root/'target/debug/dome-compositor')),'--backend','nested'],'compositor',env)
    def get(path):
        value=json.load(urllib.request.urlopen(base+path,timeout=2))
        if path=='/api/state':(out/'last-state.json').write_text(json.dumps(value))
        return value
    wait(lambda:get('/api/state')['scene']['source']=='wayland')
    xd=display.Display(env['DISPLAY']);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.5)
    def key(name):
        code=xd.keysym_to_keycode(XK.string_to_keysym(name))
        for event in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,event,code);xd.sync();time.sleep(.025)
    def motion(x,y):
        g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+x),y=int(g.y+y));xd.sync();time.sleep(.04)
    def button(down,code=1):xtest.fake_input(xd,X.ButtonPress if down else X.ButtonRelease,code);xd.sync();time.sleep(.03)
    def click(x,y):motion(x,y);button(True);button(False);time.sleep(.08)
    def capture(name):
        g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/(name+'.png'))
    def pixel(x,y):
        raw=host.get_image(int(x),int(y),1,1,X.ZPixmap,0xffffffff);return tuple(Image.frombytes('RGB',(1,1),raw.data,'raw','BGRX').getpixel((0,0)))
    def packet(events):
        request=urllib.request.Request(base+'/api/xr-input',data=json.dumps({'events':events,'actions':[]}).encode(),headers={'Content-Type':'application/json'})
        with urllib.request.urlopen(request,timeout=2)as r:assert r.status==200
    click(962,18);wait(lambda:get('/api/xr-shell')['top']==768);capture('opened-pane');wait(lambda:pixel(930,121)==(107,171,255));capture('desktop-audio')
    check(True,'Speaker tray icon opens the shared desktop/XR audio pane')
    key('Right');check(wait(lambda:level()==(.45,False)),'Keyboard adjusts the default output volume by five percent')
    key('space');check(wait(lambda:level()==(.45,True)),'Keyboard toggles output mute without losing volume')
    key('space');wait(lambda:level()==(.45,False))
    click(1056,120);check(wait(lambda:level()==(.75,False)),'Output slider sets the requested PipeWire volume')
    motion(900,120);button(True)
    for x in range(900,1002,8):motion(x,120)
    motion(1000,120);button(False)
    check(wait(lambda:level()==(.50,False)),'Dragging the volume slider keeps the final value without queued stale updates')
    click(1144,192);check(wait(lambda:level('@DEFAULT_AUDIO_SOURCE@')==(.65,False)),'Microphone volume is controlled independently')
    click(1212,192);check(wait(lambda:level('@DEFAULT_AUDIO_SOURCE@')==(.65,True)),'Microphone mute works independently of output mute')
    click(1212,192);wait(lambda:level('@DEFAULT_AUDIO_SOURCE@')==(.65,False))
    click(920,272);time.sleep(.4)
    check(defaults().get('default.audio.sink')=='dome.test.speakers','Output group heading does not activate a device')
    click(920,368);time.sleep(.4)
    check(defaults().get('default.audio.source')=='dome.test.mic','Microphone group heading does not activate a device')
    click(920,432);check(wait(lambda:defaults().get('default.audio.source')=='dome.test.usbmic'),'Pointer selects the default microphone from the device row')
    check(wait(lambda:pixel(852,431)==(107,171,255) and pixel(852,399)!=(107,171,255)),'Active microphone marker moves to the selected device')
    click(920,400);check(wait(lambda:defaults().get('default.audio.source')=='dome.test.mic'),'Pointer can select the previous microphone again')
    click(920,304);check(wait(lambda:defaults().get('default.audio.sink')=='dome.test.headset'),'Pointer selects the default output from the device row')
    check(wait(lambda:pixel(852,303)==(107,171,255) and pixel(852,335)!=(107,171,255)),'Active output marker moves to the selected device')
    capture('device-feedback')
    command('wpctl','set-default',str(nodes()['dome.test.speakers']['id']))
    check(wait(lambda:pixel(852,335)==(107,171,255) and pixel(852,303)!=(107,171,255)),'Active marker follows a default-device change made outside the tray')
    key('Home');key('Down');key('Down');key('Return');check(wait(lambda:defaults().get('default.audio.sink')=='dome.test.headset'),'Device list switches the real WirePlumber default output')
    key('End');key('Return');check(wait(lambda:defaults().get('default.audio.source')=='dome.test.usbmic'),'Device list switches the real WirePlumber default microphone')
    capture('devices-selected');key('Escape');wait(lambda:get('/api/xr-shell')['top']==36)
    # XTEST's press/release pair yields two wheel-axis events in Xvfb/winit.
    command('wpctl','set-volume','@DEFAULT_AUDIO_SINK@','.5');time.sleep(.4);motion(962,18);button(True,4);button(False,4)
    check(wait(lambda:level()==(.60,False)),'Wheel over the closed speaker tray icon adjusts output volume')
    # XR pointer opens the same controls and operates its output slider.
    packet([{'kind':'motion','id':0,'uv':[706/1024,.5]},{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}]);wait(lambda:get('/api/xr-shell')['top']==768)
    packet([{'kind':'motion','id':0,'uv':[800/1024,121/768]},{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}])
    check(wait(lambda:level()==(.75,False)),'XR hand/mouse input controls PipeWire through the same audio pane')
    packet([{'kind':'motion','id':0,'uv':[650/1024,400/768]},{'kind':'button','button':272,'pressed':True},{'kind':'button','button':272,'pressed':False}])
    check(wait(lambda:defaults().get('default.audio.source')=='dome.test.mic'),'XR pointer selects a microphone using the grouped device rows')
    key('Escape');motion(980,18);click(962,18);wait(lambda:get('/api/xr-shell')['top']==768)
    headset=nodes()['dome.test.headset']['id'];command('pw-cli','destroy',str(headset));wait(lambda:'dome.test.headset'not in nodes())
    check(wait(lambda:defaults().get('default.audio.sink')=='dome.test.speakers'),'Disconnected output falls back through WirePlumber without stale device selection')
    time.sleep(.6);capture('output-disconnected')
    policy.terminate();policy.wait(timeout=4);pipewire.terminate();pipewire.wait(timeout=4);time.sleep(.6)
    capture('service-unavailable');key('Escape');check(wait(lambda:get('/api/xr-shell')['top']==36),'Shell remains responsive when PipeWire is unavailable')
    pipewire=start(['pipewire','-c',str(root/'scripts/audio-test.conf')],'pipewire-restarted',env);wait(lambda:Path(directory+'/pipewire-0').is_socket())
    policy=start(['wireplumber','--profile','policy'],'wireplumber-restarted',env);seed();click(962,18)
    try:wait(lambda:pixel(930,121)==(107,171,255))
    finally:capture('recovery-state')
    key('Home');key('Right')
    check(wait(lambda:level()==(.45,False)),'Audio controls reconnect after PipeWire and WirePlumber restart')
    capture('service-restored');key('Escape');xd.close()
    check(compositor.poll()is None,'Compositor survives audio hotplug and service recovery')

try:
    with tempfile.TemporaryDirectory(prefix='dome-audio-',ignore_cleanup_errors=True)as directory:os.chmod(directory,0o700);main(directory)
except Exception as error:
    checks.append({'check':str(error),'passed':False});raise
finally:
    for p in reversed(processes):
        if p.poll()is None:p.terminate()
    for p in reversed(processes):
        try:p.wait(timeout=3)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    for log in logs:log.close()
    if 'directory' in globals():shutil.rmtree(directory,ignore_errors=True)
    (out/'report.json').write_text(json.dumps({'checks':checks,'failed':sum(not c['passed']for c in checks)},indent=2))
