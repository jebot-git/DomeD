#!/usr/bin/env python3
"""Private D-Bus + Xvfb integration test. Never connects to the user's session.
Requires PySide6, PyGObject, Xvfb, and a multi-gpu dome-compositor build.
"""
import json, math, os, re, signal, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_WIVRN_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
               {**os.environ, 'DOME_WIVRN_TEST_BUS': '1'})
import PySide6
from gi.repository import Gio, GLib
import urllib.request
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/wivrn'; out.mkdir(parents=True, exist_ok=True)
checks = []
def check(value, name):
    assert value, name
    checks.append(name); print('PASS', name, flush=True)
def wait(f, timeout=10):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = f()
            if value: return value
        except (OSError, ValueError, GLib.Error): pass
        time.sleep(.04)
    raise AssertionError('Timed out: ' + str(f))
with socket.socket() as s:
    s.bind(('127.0.0.1', 0)); port = s.getsockname()[1]
base = f'http://127.0.0.1:{port}'
def get(path): return json.load(urllib.request.urlopen(base + path, timeout=2))
def post(path, value):
    return json.load(urllib.request.urlopen(urllib.request.Request(base + path,
        data=json.dumps(value).encode(), headers={'Content-Type':'application/json'}), timeout=2))
processes = []; logs = []; stopped = threading.Event()
def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w'); logs.append(log)
    p = subprocess.Popen(args, env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(p); return p
try:
  with tempfile.TemporaryDirectory(prefix='dome-wivrn-') as directory:
    runtime = Path(directory); runtime.chmod(0o700)
    rd, wr = os.pipe()
    start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp'], 'xvfb', dict(os.environ), pass_fds=(wr,))
    os.close(wr)
    with os.fdopen(rd) as f: display = ':' + f.readline().strip()
    env = {**os.environ,'DISPLAY':display,'XDG_RUNTIME_DIR':directory,
           'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_DATA_HOME':str(runtime/'data'),
           'DOME_START_XR':'0','DOME_PREVIEW_PORT':str(port),'DOME_RENDERER':'gles',
           'LIBGL_ALWAYS_SOFTWARE':'1','XDG_CURRENT_DESKTOP':'Dome','RUST_LOG':'warn',
           'XDG_SESSION_TYPE':'x11','DOME_LOGIN_SESSION':'0','DOME_TERMINAL_BACKEND':'ghostty'}
    for k in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','QT_QPA_PLATFORM','QT_PLUGIN_PATH','QT_QPA_PLATFORM_PLUGIN_PATH']: env.pop(k, None)
    compositor = start([str(root/'target/debug/dome-compositor'),'--backend','nested'], 'compositor', env)
    wait(lambda:get('/api/state')['scene']['source']=='wayland')
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    def call(method, value=None, connection=bus):
        return connection.call_sync('org.domed.Compositor','/org/domed/Compositor',
          'org.domed.Compositor',method,GLib.Variant('(s)',(value,)) if value is not None else None,
          None,Gio.DBusCallFlags.NO_AUTO_START,2000,None).unpack()[0]
    wait(lambda:json.loads(call('GetIntegrationState'))['ready'])
    check(json.loads(call('GetIntegrationState'))['backend']=='nested','Versioned integration detects the native compositor')
    check(call('Activate','shell arbitrary') is False,'Control API rejects unknown commands')
    # Real notification service deduplicates retries, including after dismissal.
    def notify(key):
        return bus.call_sync('org.freedesktop.Notifications','/org/freedesktop/Notifications',
          'org.freedesktop.Notifications','Notify',GLib.Variant('(susssasa{sv}i)',
          ('WayVR',0,'','WiVRn integration','one popup',[],{'x-dome-event-id':GLib.Variant('s',key)},5000)),
          None,Gio.DBusCallFlags.NO_AUTO_START,2000,None).unpack()[0]
    wait(lambda:json.loads(call('GetIntegrationState'))['notifications']=='domed')
    bus.call_sync('org.domed.Compositor','/org/domed/Compositor','org.domed.Compositor','ForwardNotification',GLib.Variant('(sssi)',('test-forward','WayVR','direct delivery',5000)),None,Gio.DBusCallFlags.NO_AUTO_START,2000,None)
    time.sleep(.1)
    # Same ID crosses both transports without creating another presentation.
    forwarded=notify('test-forward');check(notify('test-forward')==forwarded,'Direct WayVR forwarding shares notification deduplication')
    first = notify('test-once'); check(notify('test-once')==first,'Notification retry returns the same presentation ID')
    bus.call_sync('org.freedesktop.Notifications','/org/freedesktop/Notifications','org.freedesktop.Notifications','CloseNotification',GLib.Variant('(u)',(first,)),None,Gio.DBusCallFlags.NONE,2000,None)
    check(notify('test-once')==first,'Dismissed notification is not displayed again on retry')
    match = re.search(r'Dome Wayland socket: (\S+)', (out/'compositor.log').read_text()); assert match
    journal = out/'client.jsonl'; journal.unlink(missing_ok=True)
    client_env = {**env,'WAYLAND_DISPLAY':match[1],'QT_QPA_PLATFORM':'wayland',
      'QT_WAYLAND_CLIENT_BUFFER_INTEGRATION':'shm','LD_LIBRARY_PATH':str(Path(PySide6.__file__).parent/'Qt/lib')}
    start([sys.executable,'scripts/desktop-client.py','--name','wivrn-pointer','--journal',str(journal)],'client',client_env)
    wait(lambda:len(get('/api/state')['scene']['windows'])==1)
    def heartbeat():
        while not stopped.is_set():
            try: post('/api/xr-session',{'id':731,'visible':True,'focused':True,
               'head':{'position_m':[0,1.4,0],'orientation_xyzw':[0,0,0,1]},
               'horizontal_fov_deg':100,'mouse_viewport':[-.8,.8,-.6,.6]})
            except OSError: pass
            stopped.wait(.08)
    pulse = threading.Thread(target=heartbeat,daemon=True); pulse.start()
    wait(lambda:get('/api/xr-control')['input_realm']=='xr'); time.sleep(.8)
    quad = get('/api/xr-layout')['quads'][0]
    # All test geometry is centered and the ray enters the content, below its move strip.
    pos = quad['pose']['position_m']; delta = [pos[0],pos[1]-1.4,pos[2]]
    norm = math.sqrt(sum(x*x for x in delta)); direction = [x/norm for x in delta]
    seq = 0
    def frame(buttons=0, tracked=True, distance=100, connection=bus):
        global seq
        seq += 1
        ray = {'tracked':tracked,'origin':[0,0,0],'direction':direction,'buttons':buttons,'scroll':[0,0],'max_distance':distance}
        empty = {**ray,'tracked':False,'buttons':0}
        f = {'version':1,'sequence':seq,'time_ns':time.monotonic_ns(),'rays':[ray,empty]}
        result = json.loads(call('SubmitPointers',json.dumps(f),connection))
        time.sleep(.045)
        return result
    def events(): return [json.loads(line) for line in journal.read_text().splitlines()]
    frame(1); frame(1)
    check(not any(e['kind']=='press' for e in events()),'Acquiring a laser while pinching cannot press a window')
    frame(); feedback=frame()
    check(feedback['hits'][0] is not None,'WayVR ray hits DomeD geometry in head coordinates')
    for mask, qt in [(1,1),(2,4),(4,2)]:
        frame(mask); frame()
        wait(lambda:any(e['kind']=='release' and e.get('button')==qt for e in events()))
    check(True,'Index, middle and ring actions reach Wayland as left, middle and right mouse buttons')
    frame(1)
    other=Gio.DBusConnection.new_for_address_sync(os.environ['DBUS_SESSION_BUS_ADDRESS'],Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT|Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,None,None)
    rejected=False
    try:frame(0,connection=other)
    except GLib.Error:rejected=True
    check(rejected,'Another D-Bus sender cannot steal a held pointer')
    other.close_sync(None);frame()
    before = len(events()); frame(1); time.sleep(.35)
    wait(lambda:any(e['kind']=='release' for e in events()[before:]))
    check(True,'Expired pointer lease releases held mouse buttons')
    before=len(events()); frame(1); frame(1)
    check(not any(e['kind']=='press' for e in events()[before:]),'Lease loss requires neutral before another press')
    frame(); frame(); frame(1); before=len(events()); frame(1,False)
    wait(lambda:any(e['kind']=='release' for e in events()[before:]))
    check(True,'Tracking loss balances a held mouse press')
    frame(distance=.01); check(frame(distance=.01)['hits'][0] is None,'A nearer WayVR overlay occludes the DomeD ray')
    call('Activate','toggle_desktop'); wait(lambda:not any(get('/api/state')['scene']['presentation'].values()))
    check(wait(lambda:get('/api/xr-control')['input_realm']=='desktop'),'XR/desktop shortcut hides all presentation and returns desktop input')
    call('Activate','toggle_desktop'); wait(lambda:get('/api/xr-control')['input_realm']=='xr')
    check(True,'XR/desktop shortcut restores XR input')
    overlay=runtime/'ghostty-xr';overlay.mkdir()
    with socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM) as terminal:
        terminal.bind(str(overlay/'control.sock'));terminal.settimeout(2)
        call('Activate','terminal');check(terminal.recv(64)==b'toggle','Terminal shortcut toggles the existing overlay without a tab')
    call('Activate','launcher'); wait(lambda:get('/api/xr-shell')['top']>36)
    check(True,'Launcher shortcut opens the XR launch menu')
    # Closing launcher lets screenshot enter its normal XR capture path.
    call('Activate','screenshot'); wait(lambda:get('/api/xr-control')['screenshot_request'] is not None)
    check(True,'Screenshot shortcut requests an XR capture')
    call('Activate','stop'); compositor.wait(timeout=5)
    check((runtime/'dome/explicit-stop').exists(),'Explicit compositor stop persists the recovery inhibition marker')
finally:
    stopped.set()
    for p in reversed(processes):
        if p.poll() is None: p.terminate()
    for p in reversed(processes):
        try:p.wait(timeout=5)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    for f in logs:f.close()
    (out/'report.json').write_text(json.dumps(checks,indent=2)+'\n')
