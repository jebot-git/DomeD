#!/usr/bin/env python3
"""Private nested compositor: runtime rate changes, actual Wayland callbacks and monitor modes."""
import json,os,socket,subprocess,tempfile,time,urllib.request
from pathlib import Path
root=Path(__file__).resolve().parents[1]
if os.environ.get('DOME_REFRESH_TEST_BUS')!='1':
 os.execvpe('dbus-run-session',['dbus-run-session','--','python3',__file__],{**os.environ,'DOME_REFRESH_TEST_BUS':'1'})
art=root/'artifacts/xr-refresh';art.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='dome-refresh-') as temp:
 runtime=Path(temp);children=[];logs=[]
 def start(cmd,env,name,**kw):
  log=(art/name).open('w');logs.append(log);p=subprocess.Popen(cmd,env=env,stdout=log,stderr=subprocess.STDOUT,**kw);children.append(p);return p
 def wait(fn):
  end=time.monotonic()+15
  while time.monotonic()<end:
   try:
    value=fn()
    if value:return value
   except (OSError,ValueError,KeyError,StopIteration):pass
   if any(p.poll() is not None for p in children):raise AssertionError('Child exited; inspect artifacts/xr-refresh')
   time.sleep(.02)
  raise AssertionError('Timed out')
 try:
  proto='/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml'
  subprocess.run(['wayland-scanner','client-header',proto,str(runtime/'xdg-shell-client-protocol.h')],check=True)
  subprocess.run(['wayland-scanner','private-code',proto,str(runtime/'xdg-shell-protocol.c')],check=True)
  subprocess.run(['cc','-O2','-I'+temp,str(root/'scripts/refresh-client.c'),str(runtime/'xdg-shell-protocol.c'),'-lwayland-client','-o',str(runtime/'client')],check=True)
  rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp'],os.environ.copy(),'xvfb.log',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd) as f:display=':'+f.readline().strip()
  with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
  base=f'http://127.0.0.1:{port}'
  env={**os.environ,'DISPLAY':display,'XDG_RUNTIME_DIR':temp,'XDG_CONFIG_HOME':temp+'/config','XDG_DATA_HOME':temp+'/data','XDG_STATE_HOME':temp+'/state','DOME_PREVIEW_PORT':str(port),'DOME_START_XR':'0','DOME_START_IME':'0','DOME_LOGIN_SESSION':'0','DOME_RENDERER':'gles','LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','MESA_LOADER_DRIVER_OVERRIDE','GALLIUM_DRIVER']:env.pop(key,None)
  start([os.environ.get('DOME_TEST_COMPOSITOR',str(root/'target/debug/dome-compositor')),'--backend','nested'],env,'compositor.log')
  http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
  def request(path,body=None):
   with http.open(urllib.request.Request(base+path,data=None if body is None else json.dumps(body).encode(),headers={'Content-Type':'application/json'}),timeout=2) as r:return json.load(r)
  wait(lambda:request('/api/state'))
  xr=wait(lambda:next((p for p in runtime.glob('*.dome-session.json') if json.loads(p.read_text())['realm']=='xr'),None))
  start([str(runtime/'client')],{**env,'WAYLAND_DISPLAY':xr.name.removesuffix('.dome-session.json')},'client.jsonl')
  wait(lambda:any(w['app_id']=='dome-refresh-probe' for w in request('/api/state')['scene']['windows']))
  results=[]
  for hz in [72,90,120,72]:
   def heartbeat():request('/api/xr-session',{'id':123,'visible':True,'focused':True,'frame_period_ns':round(1e9/hz)})
   heartbeat();assert request('/api/xr-control')['frame_period_ns']==round(1e9/hz);time.sleep(.25);begin=time.monotonic();end=begin+1.3
   while time.monotonic()<end:heartbeat();time.sleep(.15)
   end=time.monotonic();rows=[json.loads(line) for line in (art/'client.jsonl').read_text().splitlines()]
   frames=[r for r in rows if r['kind']=='frame' and begin<=r['time']<=end];actual=len(frames)/(end-begin)
   modes={r['name']:r['refresh'] for r in rows if r['kind']=='output'}
   assert modes.get('Dome-XR')==hz*1000,modes
   assert modes.get('winit')==60000,('Physical output timing changed',modes)
   assert hz*.78<actual<hz*1.18,('XR callbacks did not follow runtime',hz,actual)
   results.append({'announced_hz':hz,'measured_hz':round(actual,2),'output_millihz':modes['Dome-XR']})
  request('/api/xr-session',{'id':123,'visible':False})
  time.sleep(.2);begin=time.monotonic();time.sleep(.5)
  rows=[json.loads(line) for line in (art/'client.jsonl').read_text().splitlines()]
  assert sum(r['kind']=='frame' and r['time']>=begin for r in rows)>15,'Callbacks stalled after XR stopped'
  (art/'results.json').write_text(json.dumps(results,indent=2)+'\n')
  print('PASS: runtime 72→90→120→72 Hz, actual Wayland callbacks, unchanged physical output, desktop fallback',results)
 finally:
  for p in reversed(children):p.terminate()
  for p in reversed(children):
   try:p.wait(timeout=5)
   except subprocess.TimeoutExpired:p.kill();p.wait()
  for log in logs:log.close()
