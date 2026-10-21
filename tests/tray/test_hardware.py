#!/usr/bin/env python3
"""Optional hardware smoke on the caller's display, using private fake services.
No keyboard/pointer injection and no real wallet/device/configuration access.
"""
import os,sys,tempfile,time,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1'
assert os.environ.get('WAYLAND_DISPLAY') or os.environ.get('DISPLAY')
with tempfile.TemporaryDirectory(prefix='tray-gpu-') as directory:
 runtime=Path(directory);out=ROOT/'artifacts/tray-frontends';out.mkdir(exist_ok=True)
 env={**os.environ,'TRAY_FIXTURE_READY':str(runtime/'ready'),'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_CURRENT_DESKTOP':'Dome','DE_XR_GL_TRACE':'1'}
 for key in ['LIBGL_ALWAYS_SOFTWARE','GALLIUM_DRIVER','WAYLAND_SOCKET']:env.pop(key,None)
 env['FLTK_BACKEND']='wayland' if env.get('WAYLAND_DISPLAY') else 'x11'
 fixture=subprocess.Popen([sys.executable,str(ROOT/'tests/tray/mock_services.py')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 app=None
 try:
  for _ in range(100):
   if (runtime/'ready').exists():break
   assert fixture.poll() is None;time.sleep(.05)
  with (out/'hardware-frontend.log').open('w') as log:
   app=subprocess.Popen([str(ROOT/'build/tray/dome-tray'),'--devices'],env=env,stdout=log,stderr=log)
   for _ in range(100):
    text=(out/'hardware-frontend.log').read_text()
    if 'FLTK GPU widgets:' in text:break
    assert app.poll() is None;time.sleep(.05)
  renderer=next(line for line in text.splitlines() if 'FLTK GPU widgets:' in line)
  assert 'llvmpipe' not in renderer.lower() and 'softpipe' not in renderer.lower(),renderer
  print('PASS',renderer)
 finally:
  for p in [app,fixture]:
   if p is not None:
    p.terminate()
    try:p.wait(timeout=4)
    except subprocess.TimeoutExpired:p.kill();p.wait()
