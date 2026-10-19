#!/usr/bin/env python3
"""Isolated nested compositor integration test. Requires Weston and Mesa egltri_wayland.
Uses headless Weston, a private runtime directory, and a real protocol client.
Optional DOME_WESTON, DOME_WESTON_SHELL override relocated test executables/modules.
"""
import os,re,shutil,socket,subprocess,tempfile,time,json,urllib.request
from pathlib import Path
root=Path(__file__).resolve().parents[1]
weston=os.environ.get('DOME_WESTON',shutil.which('weston') or '')
client=shutil.which('egltri_wayland')
if not weston or not client:raise SystemExit('Install Weston and mesa-utils-extra (egltri_wayland) first')
with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
with tempfile.TemporaryDirectory(prefix='dome-native-')as directory:
    runtime=Path(directory);runtime.chmod(0o700)
    env={**os.environ,'XDG_RUNTIME_DIR':directory,'WAYLAND_DISPLAY':'dome-test-host','DISPLAY':'','LIBGL_ALWAYS_SOFTWARE':'1','DOME_PREVIEW_PORT':str(port),'RUST_LOG':'warn'}
    host_args=[weston,'--backend=headless','--renderer=pixman','--socket=dome-test-host','--idle-time=0','--width=1440','--height=1000','--no-config']
    if shell:=os.environ.get('DOME_WESTON_SHELL'):host_args+=['--shell='+shell]
    processes=[]
    logs={name:(runtime/(name+'.log')).open('w')for name in ['host','compositor','client']}
    def spawn(args,name,env):
        p=subprocess.Popen(args,env=env,stdout=logs[name],stderr=subprocess.STDOUT);processes.append(p);return p
    def until(predicate):
        for _ in range(200):
            if predicate():return
            if any(p.poll()is not None for p in processes):raise AssertionError('A test process exited early')
            time.sleep(.025)
        raise AssertionError('Test timed out')
    def state():return json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state'))['scene']
    def ready():
        try:state();return True
        except OSError:return False
    def action(a):
        with urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}/api/action',data=json.dumps(a).encode(),headers={'Content-Type':'application/json'}))as r:assert r.status==200
        time.sleep(.15)
    try:
        spawn(host_args,'host',env);until(lambda:(runtime/'dome-test-host').exists())
        spawn([str(root/'target/debug/dome-compositor')],'compositor',env);until(ready)
        text=(runtime/'compositor.log').read_text();match=re.search(r'Dome Wayland socket: (\S+)',text);assert match
        spawn([client],'client',{**env,'WAYLAND_DISPLAY':match[1],'WAYLAND_DEBUG':'1'})
        until(lambda:len(state()['windows'])==1)
        id=state()['windows'][0]['id'];time.sleep(.2)
        action({'op':'maximize','id':id});assert state()['windows'][0]['maximized']
        action({'op':'minimize','id':id});assert state()['windows'][0]['minimized']
        action({'op':'focus','id':id});assert not state()['windows'][0]['minimized']
        action({'op':'switch_workspace','workspace':1});assert state()['focused']is None
        action({'op':'switch_workspace','workspace':0});assert state()['focused']==id
        action({'op':'maximize','id':id});assert not state()['windows'][0]['maximized']
        log=(runtime/'client.log').read_text();assert 'ack_configure('in log;assert '.configure(720, 460,'in log;assert '.configure(1440, 904,'in log
        action({'op':'close','id':id});assert not state()['windows']
        processes[-1].wait(timeout=3)
        print('Native: real rendering client, configure acknowledgments, maximize/restore sizes, minimize, desktop switch and close passed')
    except Exception:
        for name in logs:print(name,(runtime/(name+'.log')).read_text()[-8000:])
        raise
    finally:
        for p in reversed(processes):
            if p.poll()is None:p.terminate()
        for p in reversed(processes):
            try:p.wait(timeout=3)
            except subprocess.TimeoutExpired:p.kill();p.wait()
        for f in logs.values():f.close()
