#!/usr/bin/env python3
"""Real mpvr and FLTK qimgvr surfaces in a private nested compositor.
Requires Xvfb, ffmpeg, Pillow and python-xlib; uses installed media applications.
"""
import json, math, os, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest
if os.environ.get('DOME_MEDIA_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session','--',sys.executable,__file__], {**os.environ,'DOME_MEDIA_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
art=root/'artifacts/xr-media-fullscreen';art.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='dome-media-fullscreen-') as temp:
    runtime=Path(temp);runtime.chmod(0o700)
    children=[];logs=[];stop=threading.Event();heartbeat=None
    def start(command,env,name,**kwargs):
        log=(art/name).open('w');logs.append(log)
        child=subprocess.Popen(list(map(str,command)),env=env,stdout=log,stderr=subprocess.STDOUT,**kwargs);children.append(child);return child
    def wait(check,timeout=25):
        end=time.monotonic()+timeout
        while time.monotonic()<end:
            try:
                result=check()
                if result:return result
            except (OSError,ValueError,KeyError,StopIteration):pass
            if compositor.poll() is not None:raise AssertionError('Private compositor exited')
            time.sleep(.05)
        raise AssertionError('Timed out; inspect artifacts/xr-media-fullscreen')
    try:
        rd,wr=os.pipe();start(['Xvfb','-displayfd',wr,'-screen','0','1280x800x24','-nolisten','tcp','-noreset'],os.environ.copy(),'xvfb.log',pass_fds=(wr,));os.close(wr)
        with os.fdopen(rd) as pipe:screen=':'+pipe.readline().strip()
        with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        env={**os.environ,'DISPLAY':screen,'XDG_RUNTIME_DIR':temp,'XDG_CONFIG_HOME':temp+'/config','XDG_CACHE_HOME':temp+'/cache','XDG_STATE_HOME':temp+'/state','XDG_DATA_HOME':temp+'/data','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'1','DOME_SESSION_RESTORE':'0','LIBGL_ALWAYS_SOFTWARE':'1','DOME_RENDERER':'gles','RUST_LOG':'warn','FLTK_BACKEND':'wayland'}
        for name in ['XR','TRAY','KDECONNECT','POLKIT','KWALLET','IME']:env['DOME_START_'+name]='0'
        for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','FONTCONFIG_FILE']:env.pop(name,None)
        compositor=start([os.environ.get('DOME_MEDIA_TEST_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor.log')
        http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path,body=None):
            return http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=3)
        def read(path):
            with request(path) as response:return json.load(response)
        def state():return read('/api/state')['scene']
        def action(op,**values):
            request('/api/action',{'op':op,**values}).close()
            # Wake the nested host so an idle paused player receives its configure
            # before sending its own fullscreen toggle.
            xtest.fake_input(xd,X.MotionNotify,x=20+values.get('id',0),y=40)
            xd.sync();time.sleep(.3)
        wait(state)
        xr=wait(lambda:next((p for p in runtime.glob('*.dome-session.json') if json.loads(p.read_text())['realm']=='xr'),None))
        client_env={**env,'WAYLAND_DISPLAY':xr.name.removesuffix('.dome-session.json')}
        pose={'position_m':[0.,1.6,0.],'orientation_xyzw':[0.,0.,0.,1.]}
        viewport=[-.7,.8,-.65,.75]
        def pulse():request('/api/xr-session',{'id':321,'visible':True,'focused':True,'head':pose,'horizontal_fov_deg':90,'mouse_viewport':viewport}).close()
        pulse()
        def beat():
            while not stop.wait(.1):
                try:pulse()
                except OSError:pass
        heartbeat=threading.Thread(target=beat,daemon=True);heartbeat.start()
        image=runtime/'eyes_SBS.png';pixels=Image.new('RGB',(1280,360),'red');pixels.paste('blue',(640,0,1280,360));pixels.save(image)
        video=runtime/'eyes_SBS.mkv'
        subprocess.run(['ffmpeg','-v','error','-y','-loop','1','-i',str(image),'-t','1','-c:v','ffv1',str(video)],check=True)
        xd=display.Display(screen)
        host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')),None))
        host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
        def key(name):
            code=xd.keysym_to_keycode(XK.string_to_keysym(name))
            for kind in [X.KeyPress,X.KeyRelease]:xtest.fake_input(xd,kind,code);xd.sync();time.sleep(.08)
        reports=[]
        for app,command in [
            ('org.mpvr.Sbs',['/usr/local/bin/mpvr','--domed','--xr-size=1280x720','--no-config','--gpu-sw=yes','--ao=null','--pause','--loop-file=inf',video]),
            ('org.qimgvr.Sbs',['/usr/local/bin/qimgvr','--dome-present','--sbs-present',image])]:
            if os.environ.get('DOME_MEDIA_TEST_APP') and os.environ['DOME_MEDIA_TEST_APP'] not in app:continue
            app_env={**client_env,'WAYLAND_DEBUG':'1'}
            if app=='org.qimgvr.Sbs' and os.environ.get('DOME_MEDIA_TEST_FLTK'):
                command[0]='/opt/fltk-vr/bin/qimgvr'
                app_env['LD_LIBRARY_PATH']=os.environ['DOME_MEDIA_TEST_FLTK']+'/lib64:'+os.environ['DOME_MEDIA_TEST_FLTK']+'/lib:/opt/fltk-vr/toolkit/lib:/opt/fltk-vr/lib:/opt/fltk-vr/lib/bitmap'
            player=start(command,app_env,app+'.log')
            window=wait(lambda:next((w for w in state()['windows'] if w['app_id']==app),None));id=window['id']
            wait(lambda:state()['focused']==id)
            assert not window['desktop_visible'] and window['stereo']=='side_by_side'
            def eye_frame():
                with request(f'/api/xr-frame/{id}') as response:
                    width=int(response.headers['X-Dome-Width']);height=int(response.headers['X-Dome-Height']);data=response.read()
                if len(data)!=width*height*4:return False
                frame=Image.frombytes('RGBA',(width,height),data)
                left=frame.getpixel((width//4,height//2));right=frame.getpixel((width*3//4,height//2))
                if left[0]>180 and right[2]>180:return frame
            wait(eye_frame).save(art/(app+'-surface.png'))
            before=read('/api/xr-layout');assert before['fullscreen'] is None
            action('maximize',id=id)
            layout=wait(lambda:(v if (v:=read('/api/xr-layout'))['fullscreen']==id else None))
            assert len(layout['quads'])==1 and layout['quads'][0]['id']==id and layout['quads'][0]['dome'] is None
            assert not layout['shell']
            initial=layout['quads'][0]
            pose['position_m']=[.1,1.6,0.];pulse()
            moved=wait(lambda:(v if abs((v:=read('/api/xr-layout'))['quads'][0]['pose']['position_m'][0]-initial['pose']['position_m'][0]-.075)<1e-4 else None))
            pose['orientation_xyzw']=[0.,math.sin(.1),0.,math.cos(.1)];pulse()
            rotated=wait(lambda:(v if abs((v:=read('/api/xr-layout'))['quads'][0]['pose']['orientation_xyzw'][1]-math.sin(.075))<1e-4 else None))
            assert abs(rotated['quads'][0]['pose']['orientation_xyzw'][1]-pose['orientation_xyzw'][1])>.02
            # Both real clients receive the fullscreen configure and can leave/re-enter it.
            key('f' if app=='org.mpvr.Sbs' else 'F11')
            wait(lambda:read('/api/xr-layout')['fullscreen'] is None)
            key('f' if app=='org.mpvr.Sbs' else 'F11')
            wait(lambda:read('/api/xr-layout')['fullscreen']==id)
            wait(eye_frame).save(art/(app+'-fullscreen-surface.png'))
            action('minimize',id=id);wait(lambda:read('/api/xr-layout')['fullscreen'] is None)
            action('focus',id=id);wait(lambda:read('/api/xr-layout')['fullscreen']==id)
            action('maximize',id=id);wait(lambda:read('/api/xr-layout')['fullscreen'] is None)
            normal=wait(lambda:(v if (v:=read('/api/xr-layout'))['quads'][0]['dome'] is not None else None))
            reports.append({'app':app,'fullscreen':layout,'translated':moved,'rotated':rotated,'restored':normal,'client_fullscreen_requests':True,'eye_pixels':True})
            player.terminate();player.wait(timeout=8)
            wait(lambda:not any(w['id']==id for w in state()['windows']))
            pose={'position_m':[0.,1.6,0.],'orientation_xyzw':[0.,0.,0.,1.]};pulse()
        xd.close()
        (art/'report.json').write_text(json.dumps(reports,indent=2)+'\n')
        print('PASS: real mpvr and FLTK qimgvr, compositor maximize, client fullscreen restore/re-enter, separate eye pixels, partial translation/rotation, minimize/restore and dome restoration')
    finally:
        stop.set()
        if heartbeat:heartbeat.join(timeout=4)
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
                try:child.wait(timeout=5)
                except subprocess.TimeoutExpired:child.kill();child.wait()
        for log in logs:log.close()
