#!/usr/bin/env python3
"""Private DomeD screenshot test: selection, exact pixels, XDG storage and PNG clipboard.
Run: uv run --no-project --with python-xlib --with pillow python scripts/test-screenshots.py
"""
import io,json,os,re,signal,socket,struct,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from PIL import Image,ImageChops
from Xlib import X,XK,display
from Xlib.ext import xtest
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_SCREENSHOT_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_SCREENSHOT_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/screenshots'/os.environ.get('DOME_SCREENSHOT_RENDERER','gles');out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];checks=[];xd=None
http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
def wait(fn,timeout=15):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError):pass
        time.sleep(.05)
    raise AssertionError('Timed out; inspect '+str(out))
def check(ok,text):
    checks.append({'check':text,'passed':bool(ok)});print(('PASS ' if ok else 'FAIL ')+text,flush=True);assert ok,text
def start(args,name,env,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log)
    child=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT,**kw);children.append(child);return child
def request(path,data=None,kind='application/json'):
    if isinstance(data,dict):data=json.dumps(data).encode()
    with http.open(urllib.request.Request(base+path,data=data,headers={'Content-Type':kind}),timeout=3) as r:
        return r.read()
def key(name):
    code=xd.keysym_to_keycode(XK.string_to_keysym(name));assert code,name
    for event in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,event,code);xd.sync();time.sleep(.05)
def move(x,y):
    geometry=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=x+geometry.x,y=y+geometry.y);xd.sync();time.sleep(.08)
def button(kind,which=1):xtest.fake_input(xd,kind,which);xd.sync();time.sleep(.1)
def host_image():
    g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
    return Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX')
try:
 with tempfile.TemporaryDirectory(prefix='dome-screenshots-') as directory:
    runtime=Path(directory);runtime.chmod(0o700)
    home=runtime/'home';home.mkdir();config=runtime/'config';(config/'dome').mkdir(parents=True)
    pictures=home/'Obrázky';folder=pictures/'Screenshots'
    (config/'user-dirs.dirs').write_text('XDG_PICTURES_DIR="$HOME/Obrázky"\n')
    env={**os.environ,'HOME':str(home),'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(config),'XDG_DATA_HOME':str(runtime/'data'),
        'XDG_STATE_HOME':str(runtime/'state'),'XDG_CACHE_HOME':str(runtime/'cache'),'XDG_CURRENT_DESKTOP':'Dome','DOME_RENDERER':os.environ.get('DOME_SCREENSHOT_RENDERER','gles'),
        'DOME_LOGIN_SESSION':'0','DOME_SESSION_RESTORE':'0','DOME_START_XR':'0','DOME_START_TRAY':'0','DOME_START_KDECONNECT':'0','DOME_START_POLKIT':'0',
        'DOME_START_KWALLET':'0','LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
    for name in ['DISPLAY','WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(name,None)
    rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp','-noreset'],'xvfb',env,pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd) as pipe:env['DISPLAY']=':'+pipe.readline().strip()
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    base=f'http://127.0.0.1:{port}';env['DOME_PREVIEW_PORT']=str(port)
    compositor=start([os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],'compositor',env)
    wait(lambda:request('/api/state'));xd=display.Display(env['DISPLAY'])
    host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
    host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.5)
    wayland=re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text())[1]
    paste_env={**env,'WAYLAND_DISPLAY':wayland}
    move(700,400);baseline=host_image();baseline.save(out/'baseline.png')
    key('Print');time.sleep(.4)
    move(420,210);button(X.ButtonPress);move(100,10);time.sleep(.2)
    host_image().save(out/'selection.png');button(X.ButtonRelease)
    files=wait(lambda:list(folder.glob('*.png')) if folder.exists() else None)
    check(len(files)==1,'PrintScreen captures a selected region into localized XDG Pictures/Screenshots')
    captured=Image.open(files[0]);check(captured.size==(320,200),'Reverse dragging produces exact region dimensions')
    difference=ImageChops.difference(captured.convert('RGB'),baseline.crop((100,10,420,210)))
    difference.save(out/'difference.png')
    check(difference.getbbox() is None,'Capture pixels match desktop orientation and exclude the selection overlay')
    png=subprocess.check_output(['wl-paste','--type','image/png','--no-newline'],env=paste_env,timeout=5)
    check(png==files[0].read_bytes(),'PNG clipboard bytes exactly match the saved capture')
    check(files[0].stat().st_mode&0o777==0o600,'Screenshot files are private and readable by the user')
    key('Print');time.sleep(.2);key('Escape');time.sleep(.3)
    check(len(list(folder.glob('*.png')))==1,'Escape cancels without saving or changing the clipboard')
    key('Print');time.sleep(.2);move(300,300);button(X.ButtonPress);button(X.ButtonRelease);time.sleep(.3)
    check(len(list(folder.glob('*.png')))==1,'A zero-area selection creates no file')
    subprocess.run(['gdbus','call','--session','--dest','org.domed.Compositor',
        '--object-path','/org/domed/Compositor','--method','org.domed.Compositor.Screenshot'],env=env,check=True,capture_output=True)
    time.sleep(.4);move(220,220);button(X.ButtonPress);move(350,310);button(X.ButtonRelease)
    wait(lambda:len(list(folder.glob('*.png')))==2)
    check(True,'Manager session-bus screenshot action reaches the compositor region capture path')
    # The screenshot icon is the fourth built-in, directly before the keyboard.
    # Default clock label uses its 176px minimum, plus the 44px language selector.
    move(host.get_geometry().width-234,17);button(X.ButtonPress);button(X.ButtonRelease);time.sleep(.4);move(220,220);button(X.ButtonPress);move(350,310);button(X.ButtonRelease)
    wait(lambda:len(list(folder.glob('*.png')))==3)
    check(len({f.name for f in folder.glob('*.png')})==3,'The screenshot tray icon starts capture and repeated captures use unique filenames')
    # A fresh visible XR session routes physical PrintScreen through the XR realm.
    request('/api/xr-session',{'id':741,'visible':True,'focused':True})
    time.sleep(.15)
    if json.loads(request('/api/xr-control'))['input_realm']!='xr':
        code=xd.keysym_to_keycode(XK.string_to_keysym('Super_L'))
        xtest.fake_input(xd,X.KeyPress,code);xd.sync();key('x');xtest.fake_input(xd,X.KeyRelease,code);xd.sync()
    check(wait(lambda:json.loads(request('/api/xr-control'))['input_realm']=='xr'),'XR focus selects the spatial capture path')
    request('/api/xr-session',{'id':741,'visible':True,'focused':True})
    key('Print')
    capture_id=wait(lambda:json.loads(request('/api/xr-control')).get('screenshot_request'))
    check(capture_id>0,'PrintScreen in XR requests an SBS capture instead of region selection')
    image=Image.new('RGBA',(4,2),(255,0,0,255));image.paste((0,0,255,255),(2,0,4,2));encoded=io.BytesIO();image.save(encoded,format='PNG')
    request('/api/screenshot/xr/'+str(capture_id),encoded.getvalue(),'image/png')
    stereo=wait(lambda:list(folder.glob('*_SBS.png')))
    check(stereo[0].read_bytes()==encoded.getvalue(),'XR result is saved as SBS PNG in the same screenshots folder')
    png=subprocess.check_output(['wl-paste','--type','image/png','--no-newline'],env=paste_env,timeout=5)
    check(png==encoded.getvalue(),'XR capture is also published to the session PNG clipboard')
    # A matching new helper replies with GLB. Keep the older PNG renderer test
    # above: upgrades must not mislabel an old session's SBS image as a model.
    request('/api/xr-session',{'id':741,'visible':True,'focused':True})
    key('Print')
    control=wait(lambda:json.loads(request('/api/xr-control')) if json.loads(request('/api/xr-control')).get('screenshot_request') else None)
    check(control['screenshot_gltf'],'New compositor advertises spatial glTF capture to the XR helper')
    fixture=os.environ.get('DOME_SCREENSHOT_GLTF_FIXTURE')
    if fixture:model=Path(fixture).read_bytes()
    else:
        # Minimal bounded transport fixture; the Rust export suite covers meshes.
        document=json.dumps({'asset':{'version':'2.0'},'buffers':[{'byteLength':4}]},separators=(',',':')).encode()
        document+=b' '*((-len(document))%4)
        model=b'glTF'+struct.pack('<4I',2,28+len(document)+4,len(document),0x4e4f534a)+document+struct.pack('<2I',4,0x004e4942)+b'\0'*4
    request('/api/screenshot/xr/'+str(control['screenshot_request']),model,'model/gltf-binary')
    spatial=wait(lambda:list(folder.glob('*_XR.glb')))
    check(spatial[0].read_bytes()==model,'XR glTF result is saved intact as GLB instead of another SBS image')
    check(len(list(folder.glob('*_SBS.png')))==1,'Spatial capture creates no additional SBS PNG file')
    check(spatial[0].stat().st_mode&0o777==0o600,'XR model capture uses private file permissions')
    copied=subprocess.check_output(['wl-paste','--type','model/gltf-binary','--no-newline'],env=paste_env,timeout=5)
    check(copied==model,'XR clipboard offers the exact saved model with its glTF MIME type')
    # Invalid data, stale replies and helper errors must not create bogus models.
    try:request('/api/screenshot/xr/'+str(control['screenshot_request']),model,'model/gltf-binary');assert False,'stale result accepted'
    except urllib.error.HTTPError as e:check(e.code==400,'Duplicate/stale XR capture results are rejected')
    for payload,kind in [(b'glTF broken','model/gltf-binary'),(b'capture fixture unavailable','text/plain')]:
        request('/api/xr-session',{'id':741,'visible':True,'focused':True});key('Print')
        current=wait(lambda:json.loads(request('/api/xr-control')).get('screenshot_request'))
        request('/api/screenshot/xr/'+str(current),payload,kind);time.sleep(.3)
        check(len(list(folder.glob('*_XR.glb')))==1,'Malformed model or renderer error does not save a partial capture')
    copied=subprocess.check_output(['wl-paste','--type','model/gltf-binary','--no-newline'],env=paste_env,timeout=5)
    check(copied==model,'A failed XR capture preserves the previous clipboard model')
    check(compositor.poll() is None,'Compositor remains healthy after desktop and XR captures')
finally:
    (out/'report.json').write_text(json.dumps(checks,indent=2)+'\n')
    if xd:xd.close()
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
            try:child.wait(timeout=5)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for log in logs:log.close()
