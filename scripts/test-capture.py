#!/usr/bin/env python3
"""Real Wayland → DomeD DMA-BUF → EGL eye → VA-API → decoded-file test.
Uses a private compositor and synthetic tracking; never opens a headset.
Build prototypes/capture and cargo build --features nested,xr first.
"""
import json, os, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_CAPTURE_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session','--',sys.executable,*sys.argv],
              {**os.environ,'DOME_CAPTURE_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/capture/integration';out.mkdir(parents=True,exist_ok=True)
build=Path(os.environ.get('DOME_CAPTURE_BUILD',root/'build/capture'))
encoder=os.environ.get('DOME_CAPTURE_TEST_ENCODER','vaapi')
codec=os.environ.get('DOME_CAPTURE_TEST_CODEC','h264')
children=[];logs=[];checks=[];pulse=threading.Event();thread=None
def check(value,name):
    assert value,name
    checks.append(name);print('PASS '+name,flush=True)
def start(args,name,env,**kwargs):
    log=(out/(name+'.log')).open('w');logs.append(log)
    p=subprocess.Popen(list(map(str,args)),cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,**kwargs)
    children.append(p);return p
def wait(fn,timeout=20):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError,StopIteration):pass
        time.sleep(.04)
    raise AssertionError('Timed out; see artifacts/capture/integration')
try:
    with tempfile.TemporaryDirectory(prefix='dome-capture-integration-') as directory:
        runtime=Path(directory);runtime.chmod(0o700)
        rd,wr=os.pipe()
        start(['Xvfb','-displayfd',wr,'-screen','0','1280x900x24','-nolisten','tcp'],'xvfb',os.environ.copy(),pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:display=':'+pipe.readline().strip()
        with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        env={**os.environ,'DISPLAY':display,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),
             'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),'HOME':str(runtime/'home'),
             'DOME_PREVIEW_PORT':str(port),'DOME_START_XR':'0','DOME_START_TRAY':'0','DOME_START_IME':'0',
             'DOME_LOGIN_SESSION':'0','DOME_RENDERER':'vulkan','RUST_LOG':'warn',
             'DBUS_SYSTEM_BUS_ADDRESS':'unix:path='+str(runtime/'no-system-bus'),
             'XR_RUNTIME_JSON':str(runtime/'no-runtime.json')}
        for key in ('WAYLAND_DISPLAY','WAYLAND_SOCKET','LIBGL_ALWAYS_SOFTWARE','GALLIUM_DRIVER','MESA_LOADER_DRIVER_OVERRIDE'):
            env.pop(key,None)
        http=urllib.request.build_opener(urllib.request.ProxyHandler({}));base=f'http://127.0.0.1:{port}'
        def request(path,data=None):
            with http.open(urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),
                headers={'Content-Type':'application/json'}),timeout=2) as response:return json.load(response)
        start([os.environ.get('DOME_TEST_COMPOSITOR',root/'target/debug/dome-compositor'),'--backend','nested'],'compositor',env)
        wait(lambda:request('/api/state'))
        before=request('/api/capture/xr')
        check(not before['available'] and 'gpu_socket' not in before,'Idle XR API exposes no capture surfaces')
        def xr_display():
            return next(str(p).removesuffix('.dome-session.json') for p in runtime.glob('*.dome-session.json')
                        if json.loads(p.read_text())['realm']=='xr')
        client_env={**env,'WAYLAND_DISPLAY':wait(xr_display)}
        start([build/'capture-client'],'client',client_env)
        wait(lambda:next(w for w in request('/api/state')['scene']['windows'] if w['app_id']=='org.fsvxr.fsvr.Sbs'))
        tracking={'id':774,'visible':True,'focused':True,
                  'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},
                  'eyes':[{'position_m':[x,1.4,0],'orientation_xyzw':[0,0,0,1]}for x in (-.032,.032)],
                  'horizontal_fov_deg':90,'mouse_viewport':[-.7,.7,-.7,.7]}
        def heartbeat():
            while not pulse.wait(.1):
                try:request('/api/xr-session',tracking)
                except OSError:return
        request('/api/xr-session',tracking)
        thread=threading.Thread(target=heartbeat);thread.start()
        snapshot=wait(lambda:(s if(s:=request('/api/capture/xr'))['available']else None))
        check(any(s['stereo'] for s in snapshot['surfaces']),'Capture API supplies stereo surface geometry and both runtime eye poses')
        for eye,revoke in [('left',False),('right',False),('left',True)]:
            name='revoked' if revoke else eye
            video=out/(name+'.mkv');report=out/(name+'.json')
            video.unlink(missing_ok=True);report.unlink(missing_ok=True)
            recorder=start([build/'dome-capture','--record','--source','xr','--encoder',encoder,'--codec',codec,'--eye',eye,
                '--server',base,'--width',os.environ.get('DOME_CAPTURE_TEST_WIDTH','640'),'--fps','60','--seconds','20'if revoke else'1.2',
                '--output',video,'--report',report],name,env)
            if revoke:
                time.sleep(.8);pulse.set();thread.join();tracking['visible']=False
                request('/api/xr-session',tracking)
            wait(lambda:recorder.poll() is not None)
            result=json.loads(report.read_text())
            check(result['encoder']==encoder and result['frames']>=8 and ('error'in result)==revoke,
                  name+': actual GPU recording finishes with the expected source state')
            probe=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-of','json',video]))
            check(probe['streams'][0]['codec_name']==('hevc'if codec=='h265'else codec),name+': finalized '+codec+' Matroska is decodable')
            pixels=subprocess.check_output(['ffmpeg','-v','error','-sseof','-0.15','-i',video,'-frames:v','1',
                '-f','rawvideo','-pix_fmt','rgb24','-'])
            red=sum(pixels[i]>140 and pixels[i]>pixels[i+2]*1.5 and pixels[i+1]<130 for i in range(0,len(pixels)-2,3))
            blue=sum(pixels[i+2]>140 and pixels[i+2]>pixels[i]*1.5 and pixels[i+1]<130 for i in range(0,len(pixels)-2,3))
            check((red>max(100,blue*2))if eye=='left'else(blue>max(100,red*2)),
                  name+': decoded recording contains the selected eye of the actual Wayland SBS client')
        check(not request('/api/capture/xr')['available'],'XR revocation removes capture geometry')
finally:
    pulse.set()
    if thread:thread.join(timeout=3)
    for p in reversed(children):
        if p.poll() is None:p.terminate()
    for p in reversed(children):
        try:p.wait(timeout=4)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    for log in logs:log.close()
    (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
