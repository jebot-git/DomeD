#!/usr/bin/python3
"""Private native Wayland regression for mode-aware links and reused windows."""
import json, os, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_LINK_TEST_BUS') != '1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_LINK_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/link-routing';out.mkdir(parents=True,exist_ok=True)
children=[];logs=[];stop=threading.Event();checks=[]
def start(args,env,name,**kw):
    log=(out/(name+'.log')).open('w');logs.append(log)
    child=subprocess.Popen(list(map(str,args)),env=env,stdout=log,stderr=log,**kw);children.append(child);return child
def wait(fn,seconds=20):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,StopIteration):pass
        time.sleep(.05)
    raise AssertionError('Timed out; inspect '+str(out))
def check(value,name):
    assert value,name
    checks.append(name);print('PASS '+name,flush=True)
try:
    with tempfile.TemporaryDirectory(prefix='dome-links-') as directory:
        runtime=Path(directory);runtime.chmod(0o700)
        rd,wr=os.pipe();start(['Xvfb','-displayfd',wr,'-screen','0','1280x800x24','-nolisten','tcp'],os.environ.copy(),'xvfb',pass_fds=(wr,));os.close(wr)
        with os.fdopen(rd) as stream:display=':'+stream.readline().strip()
        with socket.socket() as server:server.bind(('127.0.0.1',0));port=server.getsockname()[1]
        env={**os.environ,'DISPLAY':display,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':directory+'/config','XDG_DATA_HOME':directory+'/data','XDG_STATE_HOME':directory+'/state','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_SESSION_RESTORE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','FLTK_BACKEND':'wayland'}
        for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
        for service in ['TRAY','KWALLET','KDECONNECT','IME','POLKIT']:env['DOME_START_'+service]='0'
        start([os.environ.get('DOME_LINK_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],env,'compositor')
        http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path,data=None):
            with http.open(urllib.request.Request(f'http://127.0.0.1:{port}'+path,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json'}),timeout=3)as response:return json.load(response)
        def state():return request('/api/state')['scene']
        wait(state)
        contexts={json.loads(p.read_text())['realm']:str(p).removesuffix('.dome-session.json')for p in runtime.glob('*.dome-session.json')}
        def heartbeat():
            while not stop.wait(.08):
                try:request('/api/xr-session',{'id':7201,'visible':True,'focused':True,'head':{'position_m':[0,1.6,0],'orientation_xyzw':[0,0,0,1]},'horizontal_fov_deg':90})
                except OSError:pass
        threading.Thread(target=heartbeat,daemon=True).start()
        wait(lambda:request('/api/xr-control')['input_realm']=='xr')
        source=runtime/'client.cpp';source.write_text('#include <FL/Fl.H>\n#include <FL/Fl_Window.H>\nint main(int argc,char**argv){Fl_Window w(480,320,argv[1]);w.xclass(argv[1]);w.show();return Fl::run();}\n')
        flags=subprocess.check_output(['pkg-config','--cflags','--libs','cairo'],text=True).split()
        subprocess.run(['g++','-std=c++17','-O2','-I/opt/fltk-vr/toolkit/include',str(source),'-L/opt/fltk-vr/toolkit/lib','-Wl,-rpath,/opt/fltk-vr/toolkit/lib','-lfltk',*flags,'-o',str(runtime/'client')],check=True)
        app_ids=['org.domed.Browser','io.github.c9dev.PenguinMail']
        for i,app_id in enumerate(app_ids):start([runtime/'client',app_id],{**env,'WAYLAND_DISPLAY':contexts['xr']},'client-'+str(i))
        wait(lambda:len(state()['windows'])==2)
        recorder=runtime/'record.py';recorded=runtime/'record.json'
        recorder.write_text('import json,os,sys\nfrom pathlib import Path\nPath(sys.argv[1]).write_text(json.dumps({"socket":os.environ.get("WAYLAND_DISPLAY"),"fd":os.environ.get("WAYLAND_SOCKET"),"arguments":sys.argv[2:]}))\n')
        urls=['https://example.invalid/a b?x=1&v=ü#fragment','mailto:test@example.invalid?subject=a b','--not-a-shell-command=$(false)']
        def launch(app_id,source):
            subprocess.run([sys.executable,str(root/'packaging/dome-link'),'--app-id',app_id,'--',sys.executable,str(recorder),str(recorded),*urls],env={**env,'WAYLAND_DISPLAY':contexts[source],'WAYLAND_SOCKET':'123'},check=True,timeout=10)
            return json.loads(recorded.read_text())
        for mode,source in [('desktop','xr'),('xr','desktop'),('desktop','xr')]:
            request('/api/action',{'op':'xr_presentation','desktop':mode=='xr'})
            wait(lambda:request('/api/xr-control')['input_realm']==mode)
            for app_id in app_ids:
                value=launch(app_id,source)
                window=wait(lambda:next(w for w in state()['windows']if w['app_id']==app_id))
                check(value['socket']==contexts[mode]and value['fd']is None and value['arguments']==urls,
                    f'{app_id}: {mode} links override stale {source} socket and preserve literal URI arguments')
                check(window['desktop_visible']==(mode=='desktop')and window['xr_visible']==(mode=='xr'),
                    f'{app_id}: reused window follows {mode} mode while headset remains connected')
            check(request('/api/xr-control')['visible'],'XR remains running during '+mode+' activation')
        probe = runtime/'image-probe.py'
        probe.write_text('import sys\n' + 'sys.path.insert(0,' + repr(str(root.parent/'arc-sbs/src')) + ')\nfrom arc_sbs.domed import DomeDOutput\nprint(int(DomeDOutput().connected()))\n')
        for mode in ['desktop','xr']:
            request('/api/action',{'op':'xr_presentation','desktop':mode=='xr'})
            wait(lambda:request('/api/xr-control')['input_realm']==mode)
            result=subprocess.check_output([sys.executable,str(probe)],env={**env,'WAYLAND_DISPLAY':contexts['desktop']},text=True).strip()
            check(result==('1'if mode=='xr'else'0'),'Shared image presenter auto detection follows '+mode+' working mode with XR still available')
        # Direct mpvr calls also obey working mode, without relying on the
        # installed MIME wrapper. Exercise real native decoding/Wayland windows.
        media=runtime/'mode test.mkv'
        subprocess.run(['ffmpeg','-v','error','-y','-f','lavfi','-i','color=c=blue:s=320x180:r=12','-t','1','-c:v','ffv1',str(media)],check=True)
        launcher=root.parent/'mpvr/xr/mpvr.py'
        for mode,source in [('desktop','xr'),('xr','desktop')]:
            request('/api/action',{'op':'xr_presentation','desktop':mode=='xr'})
            wait(lambda:request('/api/xr-control')['input_realm']==mode)
            before={w['id']for w in state()['windows']}
            player=start([sys.executable,launcher,'--xr-size=640x360','--no-config','--pause','--gpu-sw=yes','--ao=null','--loop-file=inf',media],
                {**env,'WAYLAND_DISPLAY':contexts[source],'MPVR_NATIVE_PLAYER':'/opt/fltk-vr/bin/mpv','DOME_BDF_FONT_TOOL':'/usr/local/bin/dome-bdf-font'},'mpvr-'+mode)
            window=wait(lambda:next((w for w in state()['windows']if w['id']not in before and w['app_id']in ('mpv','org.mpvr.Sbs')),None),30)
            check(window['origin']==mode and window['desktop_visible']==(mode=='desktop'),
                'Real mpvr automatic playback uses '+mode+' mode despite stale '+source+' source socket')
            if mode=='xr':
                check(window['app_id']=='org.mpvr.Sbs'and window['stereo']=='side_by_side','Real XR mpvr retains packed stereo transport')
            player.terminate();player.wait(timeout=10)
            wait(lambda:all(w['id']!=window['id']for w in state()['windows']))
        # Explicit per-launch placement still wins over the current mode.
        # A browser menu activation has no URI; preserve its verified launch
        # socket even when input ownership is in the other realm.
        for mode in ['desktop','xr']:
            request('/api/action',{'op':'xr_presentation','desktop':mode=='xr'})
            wait(lambda:request('/api/xr-control')['input_realm']==mode)
            source='xr' if mode=='desktop' else 'desktop'
            subprocess.run([sys.executable,str(root/'packaging/dome-link'),'--realm','caller','--app-id',app_ids[0],'--',sys.executable,str(recorder),str(recorded)],env={**env,'WAYLAND_DISPLAY':contexts[source],'WAYLAND_SOCKET':'123'},check=True,timeout=10)
            value=json.loads(recorded.read_text());window=next(w for w in state()['windows']if w['app_id']==app_ids[0])
            check(value['socket']==contexts[source] and value['fd']is None,'Caller activation preserves '+source+' launch socket despite '+mode+' input ownership')
            check(window['desktop_visible']==(source=='desktop') and window['xr_visible']==(source=='xr'),'Caller activation presents reused browser on '+source)
        subprocess.run([sys.executable,str(root/'packaging/dome-link'),'--realm','desktop','--',sys.executable,str(recorder),str(recorded),*urls],env={**env,'WAYLAND_DISPLAY':contexts['xr']},check=True)
        check(json.loads(recorded.read_text())['socket']==contexts['desktop'],'Explicit desktop presentation overrides XR working mode')
        # An unrelated compositor/context must receive the original environment.
        subprocess.run([sys.executable,str(root/'packaging/dome-link'),'--',sys.executable,str(recorder),str(recorded),*urls],env={**env,'WAYLAND_DISPLAY':'foreign-display','WAYLAND_SOCKET':'123'},check=True)
        value=json.loads(recorded.read_text());check(value['socket']=='foreign-display'and value['fd']=='123','Foreign/missing sessions keep the original display and fd')
        (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
finally:
    stop.set()
    for child in reversed(children):
        if child.poll()is None:
            child.terminate()
            try:child.wait(timeout=4)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for log in logs:log.close()
