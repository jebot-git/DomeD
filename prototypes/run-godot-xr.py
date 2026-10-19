#!/usr/bin/env python3
"""Run Dome's OpenXR geometry viewer with an isolated Monado simulated HMD.
WiVRn mode uses the existing server and requires an already connected headset client.
No runtime manifest, pairing policy, service environment or installed files are changed.
"""
import argparse,json,os,shutil,socket,subprocess,tempfile,time,urllib.request
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--godot',default=shutil.which('godot')or str(Path.home()/'.local/bin/Godot_v4.7.2-stable_linux.x86_64'))
p.add_argument('--runtime',choices=['monado','wivrn'],default='monado')
p.add_argument('--live',action='store_true',help='Leave the viewer running until its window is closed or Ctrl+C')
p.add_argument('--scene-url',help='Use an existing Dome geometry service instead of a private sample scene')
p.add_argument('--timeout',type=int,default=120)
a=p.parse_args();root=Path(__file__).resolve().parents[1];out=root/'artifacts'/(('simulated-xr'if a.runtime=='monado'else'wivrn-xr')+('-live'if a.live else''));out.mkdir(parents=True,exist_ok=True)

def wivrn_status():
    state={}
    for prop in ['HeadsetConnected','SessionRunning']:
        r=subprocess.run(['busctl','--user','get-property','io.github.wivrn.Server','/io/github/wivrn/Server','io.github.wivrn.Server',prop],capture_output=True,text=True,timeout=3)
        state[prop]=r.stdout.strip()=='b true' if r.returncode==0 else None
    return state
try:status=wivrn_status()
except (OSError,subprocess.TimeoutExpired):status={'HeadsetConnected':None,'SessionRunning':None}
(out/'wivrn-status.json').write_text(json.dumps(status,indent=2)+'\n')
if a.runtime=='wivrn'and not status['HeadsetConnected']:
    print('WiVRn cannot present: no headset client is connected. A Monado simulated HMD is not a WiVRn network client.');raise SystemExit(2)
if not a.live:(out/'report.json').unlink(missing_ok=True)
processes=[];logs=[]
def spawn(args,name,env,stdin=None):
    log=(out/(name+'.log')).open('w');logs.append(log)
    child=subprocess.Popen(args,cwd=root,env=env,stdin=stdin,stdout=log,stderr=subprocess.STDOUT);processes.append(child);return child

def ready(predicate):
    for _ in range(400):
        if predicate():return
        if any(c.poll()is not None for c in processes):raise RuntimeError('A child process exited; see '+str(out))
        time.sleep(.025)
    raise RuntimeError('Startup timed out; see '+str(out))

def scene_ready(url):
    try:
        with urllib.request.urlopen(url+'/api/xr-scene',timeout=1)as response:return bool(json.load(response)['panels'])
    except (OSError,ValueError):return False

result=1
try:
    with tempfile.TemporaryDirectory(prefix='dome-xr-')as directory:
        env=os.environ.copy()
        if a.scene_url:url=a.scene_url.rstrip('/')
        else:
            with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            url=f'http://127.0.0.1:{port}'
            spawn([str(root/'target/debug/dome-preview')],'scene',{**env,'DOME_PREVIEW_PORT':str(port)})
        ready(lambda:scene_ready(url))
        if a.runtime=='monado':
            runtime=Path(directory)/'runtime';runtime.mkdir(mode=0o700)
            wayland=env.get('WAYLAND_DISPLAY','')
            if wayland and not wayland.startswith('/'):wayland=str(Path(env.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/wayland)
            env.update(XDG_RUNTIME_DIR=str(runtime),WAYLAND_DISPLAY=wayland,SIMULATED_ENABLE='1',XRT_COMPOSITOR_FORCE_XCB='1',IPC_EXIT_WHEN_IDLE='false',XR_RUNTIME_JSON='/usr/share/openxr/1/openxr_monado.json')
            spawn(['monado-service'],'monado',env,subprocess.PIPE)
            ready(lambda:(runtime/'monado_comp_ipc').exists())
        else:env['XR_RUNTIME_JSON']='/usr/share/openxr/1/openxr_wivrn.json'
        env.update(DOME_URL=url,DOME_XR_OUTPUT=str(out))
        args=[a.godot,'--path',str(root/'prototypes/godot-xr'),'--audio-driver','Dummy']
        if not a.live:args+=['--','--test']
        child=spawn(args,'viewer',env)
        print(f'Dome XR running; runtime={a.runtime}; scene={url}; pid={child.pid}; results={out}',flush=True)
        try:result=child.wait(timeout=None if a.live else a.timeout)
        except subprocess.TimeoutExpired:print('XR test timed out',flush=True);result=124
        if not a.live and result==0:
            report=json.loads((out/'report.json').read_text());report['wivrn_status']=status
            report['transport']='direct-monado-simulation'if a.runtime=='monado'else'wivrn'
            text=(out/'viewer.log').read_text()
            # Installed Godot 4.7.2 emits these on otherwise successful session teardown.
            known=['XR_ERROR_SESSION_NOT_STOPPING','Attempt to disconnect a nonexistent connection',"RID allocations of type 'N9OpenXRAPI18InteractionProfileE'"]
            report['engine_shutdown_diagnostics']=[line for line in text.splitlines()if any(s in line for s in known)]
            errors=[line for line in text.splitlines()if ('SCRIPT ERROR:'in line or line.startswith('ERROR:')or line.startswith('XR_ERROR_'))and not any(s in line for s in known)]
            report['unexpected_errors']=errors
            if errors or report['failed']!=0:result=1
            (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            print(f"XR checks={len(report['checks'])}; failed={report['failed']}; unexpected_errors={len(errors)}; WiVRn={status}",flush=True)
except KeyboardInterrupt:result=130
finally:
    for child in reversed(processes):
        if child.poll()is None:
            if child.stdin:
                try:child.stdin.write(b'\n');child.stdin.flush()
                except OSError:pass
            else:child.terminate()
    for child in reversed(processes):
        try:child.wait(timeout=5)
        except subprocess.TimeoutExpired:child.kill();child.wait()
    for log in logs:log.close()
raise SystemExit(result)
