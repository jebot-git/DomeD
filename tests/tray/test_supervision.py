#!/usr/bin/env python3
"""Force crashes only in a private D-Bus/Xvfb manager and verify supervision."""
import os,signal,subprocess,sys,tempfile
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK,[])
if os.environ.get('DOME_PRIVATE_TRAY_SUPERVISION')!='1':
 os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],
  {**os.environ,'DOME_PRIVATE_TRAY_SUPERVISION':'1'})
root=Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='dome-supervision-') as directory:
 runtime=Path(directory);runtime.chmod(0o700)
 env={**os.environ,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),
  'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),
  'XDG_CURRENT_DESKTOP':'Dome','FLTK_BACKEND':'x11','LIBGL_ALWAYS_SOFTWARE':'1',
  'DOME_FLTK_PREFIX':'/opt/fltk-vr/toolkit','PATH':str(root/'build/tray')+':'+os.environ['PATH'],
  'LIBRARY_PATH':'/opt/fltk-vr/toolkit/lib64:'+str(root/'.local-deps/root/usr/lib64'),
  'PKG_CONFIG_PATH':str(root/'.local-deps/root/usr/lib64/pkgconfig')}
 for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
 rd,wr=os.pipe()
 with (root/'build/tray-supervision-xvfb.log').open('w') as log:
  xvfb=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp','-noreset'],env=env,pass_fds=(wr,),stdout=log,stderr=log)
  os.close(wr)
  try:
   with os.fdopen(rd) as pipe:env['DISPLAY']=':'+pipe.readline().strip()
   subprocess.run(['cargo','test','--release','--locked','--features','nested,xr,standalone',
    '--bin','dome-compositor','session_services::tests::restarts_manager_after_process_exit',
    '--','--ignored','--nocapture'],cwd=root,env=env,check=True,timeout=180)
  finally:
   xvfb.terminate();xvfb.wait(timeout=5)
