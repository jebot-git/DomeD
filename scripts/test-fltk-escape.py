#!/usr/bin/env python3
"""Check Escape dispatch on X11 and an isolated DomeD Wayland compositor."""
import argparse
import os
from pathlib import Path
import re
import shlex
import signal
import socket
import subprocess
import tempfile
import time

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--prefix',type=Path,required=True)
parser.add_argument('--compositor',type=Path)
parser.add_argument('--x11-only',action='store_true')
parser.add_argument('--output',type=Path)
args=parser.parse_args()
repo=Path(__file__).resolve().parents[1]
out=(args.output or repo/'artifacts/fltk-escape').resolve();out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[]

def start(command,name,env,**kwargs):
    log=(out/(name+'.log')).open('w');logs.append(log)
    p=subprocess.Popen(command,env=env,stdout=log,stderr=log,start_new_session=True,**kwargs);processes.append(p);return p

try:
    with tempfile.TemporaryDirectory(prefix='domed-escape-') as temporary:
        root=Path(temporary);root.chmod(0o700)
        library=args.prefix.resolve()/'lib/libfltk.so.1.4'
        if not library.exists():library=args.prefix.resolve()/'lib64/libfltk.so.1.4'
        subprocess.run(['c++','-std=c++17','-O2',*shlex.split(subprocess.check_output(['pkg-config','--cflags','cairo'],text=True)),'-I'+str(args.prefix.resolve()/'include'),str(repo/'scripts/fltk-escape-client.cpp'),str(library),'-Wl,-rpath,'+str(library.parent),'-o',str(root/'client')],check=True)
        rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1200x900x24','-nolisten','tcp','-noreset'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
        with os.fdopen(rd) as pipe:display=':'+pipe.readline().strip()
        env={**os.environ,'DISPLAY':display,'LD_LIBRARY_PATH':str(library.parent),'FLTK_BACKEND':'x11'}
        env.pop('WAYLAND_DISPLAY',None);env.pop('WAYLAND_SOCKET',None)
        result=subprocess.run([str(root/'client')],env=env,capture_output=True,text=True,timeout=20)
        (out/'x11.log').write_text(result.stdout+result.stderr);print(result.stdout,end='',flush=True);result.check_returncode()
        if not args.x11_only:
            with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
            env={**env,'XDG_RUNTIME_DIR':temporary,'HOME':str(root/'home'),'XDG_CONFIG_HOME':str(root/'config'),
                 'XDG_DATA_HOME':str(root/'data'),'XDG_STATE_HOME':str(root/'state'),
                 'DOME_LOGIN_SESSION':'0','DOME_START_XR':'0','DOME_START_XWAYLAND':'0','DOME_START_IME':'0',
                 'DOME_SESSION_RESTORE':'0','DOME_APPEARANCE_SERVICE':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1',
                 'DOME_PREVIEW_PORT':str(port),'RUST_LOG':'warn'}
            # The existing compositor stays on its own runtime. Only the test client uses the staged library.
            compositor_env={k:v for k,v in env.items() if k not in ('LD_LIBRARY_PATH','FLTK_BACKEND')}
            start([str(args.compositor or repo/'target/release/dome-compositor'),'--backend','nested'],'compositor',compositor_env)
            deadline=time.monotonic()+20;socket_name=None
            while time.monotonic()<deadline:
                match=re.search(r'Dome Wayland socket: (\S+)',(out/'compositor.log').read_text())
                if match:socket_name=match[1];break
                if any(p.poll() is not None for p in processes):break
                time.sleep(.05)
            assert socket_name,'Isolated compositor did not start; see '+str(out)
            result=subprocess.run([str(root/'client')],env={**env,'WAYLAND_DISPLAY':socket_name,'DISPLAY':'','FLTK_BACKEND':'wayland'},capture_output=True,text=True,timeout=20)
            (out/'wayland.log').write_text(result.stdout+result.stderr);print(result.stdout,end='',flush=True);result.check_returncode()
finally:
    for p in reversed(processes):
        if p.poll() is None:
            os.killpg(p.pid,signal.SIGTERM)
            try:p.wait(timeout=4)
            except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait()
    for log in logs:log.close()
