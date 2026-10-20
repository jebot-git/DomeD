#!/usr/bin/env python3
"""Real Fcitx5/Anthy composition on an isolated D-Bus, HOME and Xvfb display."""
import json,os,re,shutil,signal,socket,subprocess,sys,tempfile,threading,time,urllib.request
from pathlib import Path
# A test launched from Dome may inherit the compositor's blocked termination signals.
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if '--fixture' in sys.argv:
    from PySide6.QtWidgets import QApplication,QLineEdit
    from PySide6.QtCore import QTimer
    app=QApplication([]);app.setDesktopFileName('dome-ime-fixture')
    class Editor(QLineEdit):
        def keyPressEvent(self,event):
            print(json.dumps({'kind':'key_press','text':event.text(),'key':event.key()}),flush=True)
            super().keyPressEvent(event)
        def mousePressEvent(self,event):
            print(json.dumps({'kind':'press','button':event.button().value}),flush=True)
            super().mousePressEvent(event)
        def wheelEvent(self,event):
            print(json.dumps({'kind':'wheel'}),flush=True)
            super().wheelEvent(event)
        def inputMethodEvent(self,event):
            print(json.dumps({'preedit':event.preeditString(),'commit':event.commitString()},ensure_ascii=False),flush=True)
            super().inputMethodEvent(event)
    editor=Editor();editor.setWindowTitle(os.environ.get('DOME_IME_FIXTURE_TITLE','DomeD IME fixture'));editor.resize(720,180)
    editor.textChanged.connect(lambda text:print(json.dumps({'text':text},ensure_ascii=False),flush=True))
    editor.show();editor.setFocus();QTimer.singleShot(180000,app.quit);sys.exit(app.exec())
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
result=1;heartbeat_stop=threading.Event();pulse=None
try:
    directory=tempfile.mkdtemp(prefix='dome-ime-')
    runtime=Path(directory);runtime.chmod(0o700)
    rd,wr=os.pipe();start([os.environ.get('DOME_XVFB',shutil.which('Xvfb') or str(root/'.local-deps/root/usr/bin/Xvfb')),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd)as pipe:display_name=':'+pipe.readline().strip()
    with socket.socket()as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    env={**os.environ,'HOME':str(runtime/'home'),'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_RUNTIME_DIR':directory,'DISPLAY':display_name,'DOME_PREVIEW_PORT':str(port),'DOME_RENDERER':os.environ.get('DOME_RENDERER','gles'),'DOME_START_IME':'1','DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','QT_IM_MODULE':'wayland','QT_WAYLAND_TEXT_INPUT_PROTOCOL':'zwp_text_input_v3','LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
    for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','IBUS_ADDRESS','FCITX_CONFIG_HOME','FCITX_DATA_HOME','QT_QPA_PLATFORM']:env.pop(name,None)
    subprocess.run(['/usr/bin/dbus-update-activation-environment','HOME','XDG_RUNTIME_DIR','XDG_CONFIG_HOME','XDG_DATA_HOME','XDG_STATE_HOME','DISPLAY','WAYLAND_DISPLAY=','QT_QPA_PLATFORM=offscreen','XDG_CURRENT_DESKTOP=Dome','XDG_SESSION_DESKTOP=dome'],env=env,check=True)
    start([os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],'compositor',env)
    base=f'http://127.0.0.1:{port}'
    def get(path):return json.load(urllib.request.urlopen(base+path,timeout=2))
    def scene():return get('/api/state')['scene']
    wait(lambda:scene()['source']=='wayland')
    wayland=wait(lambda:re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text()))[1]
    client_env={**env,'WAYLAND_DISPLAY':wayland,'QT_QPA_PLATFORM':'wayland'}
    def owner():
        owner=subprocess.run(['/usr/bin/busctl','--address='+env['DBUS_SESSION_BUS_ADDRESS'],'call','org.freedesktop.DBus','/org/freedesktop/DBus','org.freedesktop.DBus','NameHasOwner','s','org.fcitx.Fcitx5'],env=client_env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=2)
        return b'true' in owner.stdout
    def method():
        if not owner():return ''
        r=subprocess.run([os.environ.get('DOME_FCITX5_REMOTE','/usr/bin/fcitx5-remote'),'--check','-n'],env=client_env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=2)
        return r.stdout.decode().strip() if r.returncode==0 else ''
    wait(owner)
    check(True,'DomeD starts Fcitx5 on its private trusted Wayland connection')
    def configured_methods():
        response=subprocess.run(['/usr/bin/busctl','--json=short','--address='+env['DBUS_SESSION_BUS_ADDRESS'],'call','org.fcitx.Fcitx5','/controller','org.fcitx.Fcitx.Controller1','InputMethodGroupInfo','s','Default'],env=client_env,check=True,stdout=subprocess.PIPE,timeout=2)
        return [item[0] for item in json.loads(response.stdout)['data'][1]]
    expected_methods=['keyboard-us','anthy','keyboard-cz','hangul','keyboard-rs']
    wait(lambda:all(name in configured_methods() for name in expected_methods))
    profile=runtime/'config/dome/fcitx5/profile'
    wait(lambda:profile.exists() and all('Name='+name in profile.read_text() for name in expected_methods))
    check(True,'DomeD adds installed Czech, Hangul and Serbian Cyrillic methods to its profile')
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
    def cycle_to(target):
        for _ in range(10):
            if method()==target:return
            previous=method();chord(['Super_L'],'space')
            wait(lambda:method()!=previous)
        raise AssertionError('Input method unavailable in switcher: '+target)
    def entries(name='editor'):
        return [json.loads(line)for line in (out/(name+'.log')).read_text().splitlines() if line.startswith('{')]
    def shot(name):
        g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/(name+'.png'))
    wait(lambda:scene()['focused']==window['id'])
    # Select US explicitly for reproducibility; subsequent changes use the compositor shortcut.
    subprocess.run([os.environ.get('DOME_FCITX5_REMOTE','/usr/bin/fcitx5-remote'),'-s','keyboard-us'],env=client_env,check=True);time.sleep(.3)
    shot('before-typing')
    for c in 'hello':key(c)
    wait(lambda:any(e.get('text')=='hello' for e in entries()))
    check(True,'US English input reaches a native text-input-v3 editor')
    # The compositor switcher must produce real text, not only change its label.
    language_samples=[
        ('keyboard-cz','234','ěšč','Czech'),
        ('keyboard-rs','asdqw','асдљњ','Serbian Cyrillic'),
        ('hangul','gksrmf','한글','Korean Hangul'),
    ]
    for target,keys,expected,language in language_samples:
        cycle_to('keyboard-us');chord(['Control_L'],'a');key('BackSpace')
        cycle_to(target)
        for c in keys:key(c)
        if target=='hangul':key('Return')
        wait(lambda:any(e.get('text')==expected for e in entries()))
        check(True,language+' text is entered through DomeD input switching')
        check(scene()['focused']==window['id'],language+' switching preserves the focused editor')
        shot('language-'+target)
    cycle_to('keyboard-us')
    chord(['Control_L'],'a');key('BackSpace')
    g=host.get_geometry()
    xtest.fake_input(xd,X.MotionNotify,x=g.x+g.width-198,y=g.y+18);xd.sync()
    for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,1);xd.sync();time.sleep(.08)
    wait(lambda:method()=='anthy')
    check(True,'Clicking the keyboard switcher changes US English to Anthy')
    for c in 'nihongo':key(c)
    wait(lambda:any('にほんご' in e.get('preedit','') for e in entries()))
    check(True,'Anthy produces Japanese preedit through the Wayland text-input protocol')
    key('space');key('space')
    # Learning changes candidate order between Anthy versions/runs. Select the
    # expected conversion by its preedit before exercising its popup row.
    def latest_preedit():
        return next((e['preedit'] for e in reversed(entries()) if 'preedit' in e),'')
    for _ in range(6):
        time.sleep(.15)
        if latest_preedit()=='日本語':break
        key('space')
    wait(lambda:latest_preedit()=='日本語')
    time.sleep(.4);shot('anthy-candidates')
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
    # Select the highlighted conversion. Anthy's learned candidate order varies;
    # do not assume 日本語 is always the first row.
    rgb=texture.convert('RGB');highlight=[]
    for y in range(1,rgb.height-1):
        colors=[rgb.getpixel((x,y)) for x in range(4,rgb.width-4)]
        if sum(50<min(c)<200 and max(c)-min(c)<15 for c in colors)>len(colors)*.7:highlight.append(y)
    assert highlight,'Anthy selection highlight missing from candidate surface'
    bands=[]
    for y in highlight:
        if not bands or y>bands[-1][-1]+1:bands.append([])
        bands[-1].append(y)
    band=max(bands,key=len);candidate_y=(band[0]+band[-1])/2
    xtest.fake_input(xd,X.MotionNotify,x=int(g.x+r['x']+popup['anchor']['x']+texture.width*.6),y=int(g.y+r['y']+popup['anchor']['y']+candidate_y));xd.sync()
    for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,1);xd.sync();time.sleep(.08)
    check(scene()['focused']==window['id'],'Clicking an Anthy candidate preserves text-field focus')
    key('Return');wait(lambda:any('日本語' in e.get('text','')for e in entries()))
    check(True,'Selected Anthy conversion commits 日本語 into the origin Wayland editor')
    cycle_to('keyboard-us')
    key('a');wait(lambda:any(e.get('text','').endswith('a')for e in entries()))
    check(True,'Switching back restores direct English input')
    check(scene()['focused']==window['id'],'IME candidates and switching preserve the origin window focus')
    chord(['Super_L','Shift_L'],'space');shot('launcher-shortcut');key('Escape')
    check(any('日本語a'==e.get('text') for e in entries()),'Launcher binding remains separate from input-method switching')
    # Exercise Ghostty's SDL composition display, not just Qt's built-in editor.
    ghostty_binary=Path(os.environ.get('DOME_GHOSTTY',str(root.parent/'ghostty-xr/build/ghostty-xr')))
    received=runtime/'ghostty-input';ready=runtime/'ghostty-ready'
    terminal_loop="while True:\n data=os.read(0,4096)\n f.write(data)\n os.write(1,data)"
    terminal_code="import os,tty; from pathlib import Path; tty.setraw(0); f=open(%r,'ab',buffering=0); Path(%r).touch(); exec(%r)"%(str(received),str(ready),terminal_loop)
    ghostty=start([str(ghostty_binary),'--wayland','--cols','40','--rows','8','--',sys.executable,'-c',terminal_code],'ghostty',client_env)
    terminal=wait(lambda:next((w for w in scene()['windows']if w['app_id']=='org.ghostty.xr.wayland'),None))
    wait(lambda:ready.exists() and scene()['focused']==terminal['id'])
    def terminal_frame():
        try:
            with urllib.request.urlopen(base+f"/api/xr-frame/{terminal['id']}",timeout=2)as response:return response.read()
        except urllib.error.HTTPError as error:
            if error.code==409:return None
            raise
    time.sleep(.3);blank=wait(terminal_frame)
    cycle_to('anthy')
    for c in 'nihongo':key(c)
    wait(lambda:terminal_frame()!=blank)
    check(received.read_bytes()==b'','Ghostty displays Japanese preedit before commit without sending it to the PTY')
    key('Escape');wait(lambda:terminal_frame()==blank)
    check(received.read_bytes()==b'','Cancelling Ghostty composition clears the display without sending input')
    for c in 'nihongo':key(c)
    key('space');time.sleep(.2);key('Return')
    wait(lambda:received.read_bytes()=='日本語'.encode())
    check(method()=='anthy','Ghostty commits Japanese with Enter while Anthy remains selected')
    # Longer than SDL's fixed 32-byte edit event; the entire composition must render.
    committed=wait(terminal_frame)
    for c in 'nihongo'*5:key(c)
    wait(lambda:terminal_frame()!=committed)
    check(received.read_bytes()=='日本語'.encode(),'Long Japanese composition remains local until commit')
    key('Escape');cycle_to('keyboard-us')
    ghostty.terminate();ghostty.wait(timeout=5)
    wait(lambda:not any(w['id']==terminal['id']for w in scene()['windows']))
    def post(path,value):
        with urllib.request.urlopen(urllib.request.Request(base+path,data=json.dumps(value).encode(),headers={'Content-Type':'application/json'}),timeout=2)as response:return response.status
    post('/api/action',{'op':'focus','id':window['id']})
    wait(lambda:scene()['focused']==window['id'])
    # Exercise focus restoration while Fcitx owns an input-method keyboard grab.
    def post(path,value):
        with urllib.request.urlopen(urllib.request.Request(base+path,data=json.dumps(value).encode(),headers={'Content-Type':'application/json'}),timeout=2)as response:return response.status
    def heartbeat():
        while not heartbeat_stop.is_set():
            post('/api/xr-session',{'visible':True,'focused':True,'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':100})
            heartbeat_stop.wait(.1)
    pulse=threading.Thread(target=heartbeat,daemon=True);pulse.start()
    wait(lambda:get('/api/xr-control')['input_realm']=='xr')
    xr_socket=re.search(r'Dome XR Wayland socket: (\S+)',(out/'compositor.log').read_text())[1]
    start([sys.executable,__file__,'--fixture'],'xr-editor',{**client_env,'WAYLAND_DISPLAY':xr_socket,'DOME_IME_FIXTURE_TITLE':'DomeD XR input fixture'})
    xr_window=wait(lambda:next((w for w in scene()['windows']if w['title']=='DomeD XR input fixture'),None))
    time.sleep(.5)
    post('/api/xr-input',{'events':[{'kind':'motion','id':xr_window['id'],'uv':[.5,.5]}],'actions':[{'op':'focus','id':xr_window['id']}]})
    wait(lambda:scene()['focused']==xr_window['id']);time.sleep(.2)
    key('q');time.sleep(.2)
    check(any(e.get('text')=='q'for e in entries('xr-editor')),'Shown XR receives keyboard input even with an active input-method grab')
    for cycle in range(2):
        xr_before=len(entries('xr-editor'));desktop_before=len(entries())
        chord(['Super_L','Shift_L'],'d');wait(lambda:get('/api/xr-control')['input_realm']=='desktop')
        key('w');time.sleep(.2)
        check(any(e.get('kind')=='key_press'and e.get('text')=='w'for e in entries()[desktop_before:]) and not any(e.get('kind')=='key_press'and e.get('text')=='w'for e in entries('xr-editor')[xr_before:]),'Hidden XR restores desktop typing without leaking keys to XR, cycle '+str(cycle+1))
        # Measure physical events after the realm transition and its queued client events.
        time.sleep(.2)
        desktop_before=len(entries());xr_before=len(entries('xr-editor'))
        r=next(w['desktop']for w in scene()['windows']if w['id']==window['id']);g=host.get_geometry()
        xtest.fake_input(xd,X.MotionNotify,x=int(g.x+r['x']+100),y=int(g.y+r['y']+70));xd.sync();time.sleep(.1)
        for code in [1,4]:
            for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,code);xd.sync();time.sleep(.05)
        check(wait(lambda:any(e.get('kind')=='press'for e in entries()[desktop_before:]) and any(e.get('kind')=='wheel'for e in entries()[desktop_before:]) and not any(e.get('kind')in ['press','wheel']for e in entries('xr-editor')[xr_before:])),'Hidden XR passes physical clicks and scrolling exclusively to desktop, cycle '+str(cycle+1))
        desktop_before=len(entries());xr_before=len(entries('xr-editor'))
        chord(['Super_L','Shift_L'],'d');wait(lambda:get('/api/xr-control')['input_realm']=='xr');time.sleep(.3)
        desktop_before=len(entries());xr_before=len(entries('xr-editor'))
        key('e');time.sleep(.2)
        check(any(e.get('kind')=='key_press'and e.get('text')=='e'for e in entries('xr-editor')[xr_before:]) and not any(e.get('kind')=='key_press'and e.get('text')=='e'for e in entries()[desktop_before:]),'Showing XR restores XR typing without leaking keys to desktop, cycle '+str(cycle+1))
        post('/api/xr-input',{'events':[{'kind':'motion','id':xr_window['id'],'uv':[.5,.5]}],'actions':[]})
        time.sleep(.1);pos=xd.screen().root.query_pointer();xtest.fake_input(xd,X.MotionNotify,x=pos.root_x+1,y=pos.root_y);xd.sync();time.sleep(.1)
        desktop_before=len(entries());xr_before=len(entries('xr-editor'))
        for code in [1,4]:
            for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,code);xd.sync();time.sleep(.05)
        check(wait(lambda:any(e.get('kind')=='press'for e in entries('xr-editor')[xr_before:]) and any(e.get('kind')=='wheel'for e in entries('xr-editor')[xr_before:]) and not any(e.get('kind')in ['press','wheel']for e in entries()[desktop_before:])),'Shown XR receives physical clicks and scrolling exclusively, cycle '+str(cycle+1))
    # Real Japanese conversion must follow both automatic focus changes too.
    for hidden,target,other in [(True,'editor','xr-editor'),(False,'xr-editor','editor')]:
        chord(['Super_L','Shift_L'],'d')
        wait(lambda:get('/api/xr-control')['input_realm']==('desktop'if hidden else'xr'))
        if method()!='anthy':cycle_to('anthy')
        before=len(entries(target));other_before=len(entries(other))
        for c in 'nihongo':key(c)
        key('space');key('Return')
        wait(lambda:any(e.get('commit')=='日本語'for e in entries(target)[before:]))
        check(not any(e.get('commit')=='日本語'for e in entries(other)[other_before:]),'Anthy conversion follows automatic '+('desktop'if hidden else'XR')+' focus restoration without leaking commits')
    for target,keys,expected,language in language_samples:
        cycle_to('keyboard-us');chord(['Control_L'],'a');key('BackSpace')
        desktop_before=len(entries())
        cycle_to(target)
        for c in keys:key(c)
        if target=='hangul':key('Return')
        wait(lambda:any(e.get('text')==expected for e in entries('xr-editor')))
        check(not any(e.get('text')==expected for e in entries()[desktop_before:]),language+' XR input follows focus without leaking to desktop')
    cycle_to('keyboard-us')
    heartbeat_stop.set();pulse.join(timeout=3)
    xd.close();result=0
except Exception as error:
 checks.append({'check':str(error),'passed':False})
 raise
finally:
 heartbeat_stop.set()
 if pulse:pulse.join(timeout=3)
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
