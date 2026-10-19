#!/usr/bin/env python3
"""Real Fcitx5/Anthy composition on an isolated D-Bus, HOME and Xvfb display."""
import json,os,re,shutil,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
if '--fixture' in sys.argv:
    from PySide6.QtWidgets import QApplication,QLineEdit
    from PySide6.QtCore import QTimer
    app=QApplication([]);app.setDesktopFileName('dome-ime-fixture')
    class Editor(QLineEdit):
        def inputMethodEvent(self,event):
            print(json.dumps({'preedit':event.preeditString(),'commit':event.commitString()},ensure_ascii=False),flush=True)
            super().inputMethodEvent(event)
    editor=Editor();editor.setWindowTitle('DomeD IME fixture');editor.resize(720,180)
    editor.textChanged.connect(lambda text:print(json.dumps({'text':text},ensure_ascii=False),flush=True))
    editor.show();editor.setFocus();QTimer.singleShot(90000,app.quit);sys.exit(app.exec())
if os.environ.get('DOME_IME_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_IME_TEST_BUS':'1'})
from Xlib import X,XK,display
from Xlib.ext import xtest
from PIL import Image
root=Path(__file__).resolve().parents[1];out=root/'artifacts/input-method';out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[];checks=[]
def check(ok,label):
    checks.append({'check':label,'passed':bool(ok)});print(('PASS 'if ok else'FAIL ')+label,flush=True)
    assert ok,label

def start(args,name,env,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log);p=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT,**kw);processes.append(p);return p

def wait(fn,timeout=20):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError):pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/input-method')
result=1
try:
    directory=tempfile.mkdtemp(prefix='dome-ime-')
    runtime=Path(directory);runtime.chmod(0o700)
    rd,wr=os.pipe();start([str(root/'.local-deps/root/usr/bin/Xvfb'),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as pipe:display_name=':'+pipe.readline().strip()
    with socket.socket()as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    env={**os.environ,'HOME':str(runtime/'home'),'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_RUNTIME_DIR':directory,'DISPLAY':display_name,'DOME_PREVIEW_PORT':str(port),'DOME_START_IME':'1','DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','QT_IM_MODULE':'wayland','QT_WAYLAND_TEXT_INPUT_PROTOCOL':'zwp_text_input_v3','LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
    for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','IBUS_ADDRESS','FCITX_CONFIG_HOME','FCITX_DATA_HOME','QT_QPA_PLATFORM']:env.pop(name,None)
    subprocess.run(['/usr/bin/dbus-update-activation-environment','HOME','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','DISPLAY','WAYLAND_DISPLAY=','QT_QPA_PLATFORM=offscreen','XDG_CURRENT_DESKTOP=Dome','XDG_SESSION_DESKTOP=dome'],env=env,check=True)
    start([os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],'compositor',env)
    base=f'http://127.0.0.1:{port}'
    def get(path):return json.load(urllib.request.urlopen(base+path,timeout=2))
    def scene():return get('/api/state')['scene']
    wait(lambda:scene()['source']=='wayland')
    wayland=re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text())[1]
    client_env={**env,'WAYLAND_DISPLAY':wayland,'QT_QPA_PLATFORM':'wayland'}
    def owner():
        owner=subprocess.run(['/usr/bin/busctl','--address='+env['DBUS_SESSION_BUS_ADDRESS'],'call','org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','NameHasOwner','s','org.fcitx.Fcitx5'],env=client_env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=2)
        return b'true' in owner.stdout
    def method():
        if not owner():return ''
        r=subprocess.run(['/usr/bin/fcitx5-remote','--check','-n'],env=client_env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=2)
        return r.stdout.decode().strip() if r.returncode==0 else ''
    wait(owner)
    check(True,'DomeD starts Fcitx5 on its private trusted Wayland connection')
    start([sys.executable,__file__,'--fixture'],'editor',client_env)
    window=wait(lambda:next((w for w in scene()['windows'] if 'IME fixture' in w['title']),None))
    xd=display.Display(display_name);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None))
    host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
    time.sleep(.5)
    r=next(w['desktop'] for w in scene()['windows'] if w['id']==window['id']);g=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(g.x+r['x']+100),y=int(g.y+r['y']+70));xd.sync()
    for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,1);xd.sync();time.sleep(.08)
    def key(name,down=None):
        code=xd.keysym_to_keycode(XK.string_to_keysym(name));assert code,name
        for pressed in ([True,False]if down is None else[down]):xtest.fake_input(xd,X.KeyPress if pressed else X.KeyRelease,code);xd.sync();time.sleep(.04)
    def chord(modifiers,keyname):
        for m in modifiers:key(m,True)
        key(keyname)
        for m in reversed(modifiers):key(m,False)
        time.sleep(.3)
    def entries():
        return [json.loads(line)for line in (out/'editor.log').read_text().splitlines() if line.startswith('{')]
    def shot(name):
        g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/(name+'.png'))
    wait(lambda:scene()['focused']==window['id'])
    # Select US explicitly for reproducibility; subsequent changes use the compositor shortcut.
    subprocess.run(['/usr/bin/fcitx5-remote','-s','keyboard-us'],env=client_env,check=True);time.sleep(.3)
    shot('before-typing')
    for c in 'hello':key(c)
    wait(lambda:any(e.get('text')=='hello' for e in entries()))
    check(True,'US English input reaches a native text-input-v3 editor')
    chord(['Control_L'],'a');key('BackSpace')
    chord(['Super_L'],'space');wait(lambda:method()=='anthy')
    check(True,'Super+Space switches from US English to Anthy')
    for c in 'nihongo':key(c)
    wait(lambda:any('にほんご' in e.get('preedit','') for e in entries()))
    check(True,'Anthy produces Japanese preedit through the Wayland text-input protocol')
    key('space');key('space');time.sleep(.4);shot('anthy-candidates')
    wait(lambda:len(scene().get('popups',[]))>0)
    check(True,'Anthy candidate popup participates in DomeD spatial popup layout')
    popup=scene()['popups'][0]
    def candidate_texture():
        try:
            with urllib.request.urlopen(base+f"/api/xr-frame/{popup['id']}",timeout=2) as response:
                size=(int(response.headers['X-Dome-Width']),int(response.headers['X-Dome-Height']))
                return Image.frombytes('RGBA',size,response.read())
        except urllib.error.HTTPError as error:
            if error.code==409:return None
            raise
    texture=wait(candidate_texture);texture.save(out/'anthy-candidates-xr.png')
    check(texture.width>20 and texture.height>20,'Anthy candidate surface exports a live texture for direct and dome XR views')
    # Candidates share popup positioning, while keeping the origin editor focused.
    r=next(w['desktop']for w in scene()['windows']if w['id']==window['id']);g=host.get_geometry()
    xtest.fake_input(xd,X.MotionNotify,x=int(g.x+r['x']+r['w']+22),y=int(g.y+r['y']+46));xd.sync()
    for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,1);xd.sync();time.sleep(.08)
    check(scene()['focused']==window['id'],'Clicking an Anthy candidate preserves text-field focus')
    key('Return');wait(lambda:any('日本語' in e.get('text','')for e in entries()))
    check(True,'Selected Anthy conversion commits 日本語 into the origin Wayland editor')
    chord(['Super_L'],'space');wait(lambda:method()=='keyboard-us')
    key('a');wait(lambda:any(e.get('text','').endswith('a')for e in entries()))
    check(True,'Switching back restores direct English input')
    check(scene()['focused']==window['id'],'IME candidates and switching preserve the origin window focus')
    chord(['Super_L','Shift_L'],'space');shot('launcher-shortcut');key('Escape')
    check(any('日本語a'==e.get('text') for e in entries()),'Launcher binding remains separate from input-method switching')
    xd.close();result=0
finally:
 for p in reversed(processes):
    if p.poll()is None:p.terminate()
 for p in reversed(processes):
    try:p.wait(timeout=5)
    except subprocess.TimeoutExpired:p.kill();p.wait()
 if 'directory' in globals():
    subprocess.run(['/usr/bin/fusermount3','-u',str(Path(directory)/'doc')],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    shutil.rmtree(directory,ignore_errors=True)
 for log in logs:log.close()
 (out/'report.json').write_text(json.dumps({'checks':checks,'failed':sum(not c['passed']for c in checks)},indent=2))
sys.exit(result)
