#!/usr/bin/env python3
"""Exercise window transfer with real Wayland/X11 clients on an isolated display.
Run: uv run --no-project --with python-xlib python scripts/test-realm-transfer.py
An XR heartbeat simulates the renderer; no headset or live session is touched.
"""
import json, os, re, shutil, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from pathlib import Path
from Xlib import X, XK, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_TRANSFER_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_TRANSFER_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/realm-transfer'
out.mkdir(parents=True, exist_ok=True)
children, logs, checks = [], [], []
stop = threading.Event()

def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    children.append(child)
    return child

def wait(fn, timeout=15):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            result = fn()
            if result:
                return result
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/realm-transfer')

def check(value, name):
    checks.append({'check': name, 'passed': bool(value)})
    print(('PASS ' if value else 'FAIL ') + name, flush=True)
    assert value, name

result = 1
try:
    with tempfile.TemporaryDirectory(prefix='dome-transfer-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        rd, wr = os.pipe()
        start([os.environ.get('DOME_XVFB') or shutil.which('Xvfb') or str(root / '.local-deps/root/usr/bin/Xvfb'),
               '-displayfd', str(wr), '-screen', '0', '1600x1000x24', '-nolisten', 'tcp', '-noreset'],
              'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            host_display = ':' + pipe.readline().strip()
        env = {**os.environ, 'DISPLAY': host_display, 'XDG_RUNTIME_DIR': directory,
               'HOME': str(runtime / 'home'), 'XDG_CONFIG_HOME': str(runtime / 'config'),
               'XDG_DATA_HOME': str(runtime / 'data'), 'XDG_STATE_HOME': str(runtime / 'state'),
               'DOME_LOGIN_SESSION': '1', 'DOME_START_XR': '0', 'DOME_START_IME': '0',
               'DOME_SESSION_RESTORE': '0', 'DOME_START_XWAYLAND': '1',
               'DOME_DRM_TEST_DEVICE': '/dev/null', 'DOME_APPEARANCE_SERVICE': '0',
               'DOME_PREVIEW_PORT': str(port), 'DOME_RENDERER': os.environ.get('DOME_TEST_RENDERER', 'gles'),
               'LIBGL_ALWAYS_SOFTWARE': '1', 'RUST_LOG': 'warn', 'GTK_USE_PORTAL': '0',
               'NO_AT_BRIDGE': '1', 'GIO_USE_VFS': 'local'}
        for name in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'GTK_MODULES', 'QT_QPA_PLATFORM', 'PAM_KWALLET5_LOGIN']:
            env.pop(name, None)
        if env['DOME_RENDERER'] == 'vulkan':
            env.pop('LIBGL_ALWAYS_SOFTWARE', None)
        compositor = start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/debug/dome-compositor')),
                            '--backend', 'nested'], 'compositor', env)
        base = f'http://127.0.0.1:{port}'
        def get(path):
            return json.load(urllib.request.urlopen(base + path, timeout=2))
        def post(path, value):
            with urllib.request.urlopen(urllib.request.Request(base + path, data=json.dumps(value).encode(),
                                       headers={'Content-Type': 'application/json'}), timeout=2) as response:
                return json.load(response)
        def scene():
            return get('/api/state')['scene']
        def window(id):
            return next(w for w in scene()['windows'] if w['id'] == id)
        def log():
            return (out / 'compositor.log').read_text()
        wayland = wait(lambda: re.search(r'Dome Wayland socket: (\S+)', log()))[1]
        bridge = wait(lambda: re.search(r'Dome Xwayland bridge: (:\d+)', log()))[1]
        host = display.Display(host_display)
        host_window = wait(lambda: next((w for w in host.screen().root.query_tree().children
                                  if str(w.get_wm_name() or '').startswith('Dome')), None))
        host_window.set_input_focus(X.RevertToParent, X.CurrentTime)
        host.sync()
        def transfer_key():
            codes = [host.keysym_to_keycode(XK.string_to_keysym(k)) for k in ['Super_L', 'Shift_L', 'x']]
            for code in codes:
                xtest.fake_input(host, X.KeyPress, code)
            for code in reversed(codes):
                xtest.fake_input(host, X.KeyRelease, code)
            host.sync()
            time.sleep(.2)
        def presentation(xr):
            post('/api/action', {'op': 'xr_presentation', 'desktop': xr})
            wait(lambda: get('/api/xr-control')['input_realm'] == ('xr' if xr else 'desktop'))
            time.sleep(.35)
        # A transient Wayland toplevel exercises owner routing independently of app_id.
        fixture = '''import gi
import os
gi.require_version('Gtk','3.0')
from gi.repository import Gtk,GLib
GLib.set_prgname('org.domed.TransferFixture')
a=Gtk.Window(title='Transfer Wayland owner');a.set_default_size(600,350)
a.add(Gtk.Entry(text='unsaved application state'));a.show_all()
b=Gtk.Window(title='Transfer Wayland dialog',transient_for=a);b.set_default_size(250,120);b.show_all()
Gtk.main()
'''
        client = start(['/usr/bin/python3', '-c', fixture], 'wayland-client', {**env, 'WAYLAND_DISPLAY': wayland, 'GDK_BACKEND': 'wayland'})
        owner = wait(lambda: next((w['id'] for w in scene()['windows'] if w['title'] == 'Transfer Wayland owner'), None))
        dialog = wait(lambda: next((w['id'] for w in scene()['windows'] if w['title'] == 'Transfer Wayland dialog'), None))
        wait(lambda: any(p['id'] == dialog and p['root'] == owner for p in scene()['popups']))
        post('/api/action', {'op': 'focus', 'id': dialog})
        transfer_key()
        check(window(owner)['desktop_visible'] and window(owner)['xr_visible'], 'Sending to inactive XR is disabled')
        post('/api/action', {'op': 'xr_presentation', 'desktop': False})
        def heartbeat():
            while not stop.is_set():
                post('/api/xr-session', {'visible': True, 'focused': True,
                     'head': {'position_m': [0, 1.4, 0], 'orientation_xyzw': [0, 0, 0, 1]}, 'horizontal_fov_deg': 100})
                stop.wait(.1)
        pulse = threading.Thread(target=heartbeat, daemon=True)
        pulse.start()
        wait(lambda: get('/api/xr-control')['visible'])
        time.sleep(.5)
        before = {id: window(id)['desktop'] for id in [owner, dialog]}
        post('/api/action', {'op': 'focus', 'id': dialog})
        transfer_key()
        wait(lambda: not window(owner)['desktop_visible'])
        check(not window(dialog)['desktop_visible'] and window(dialog)['xr_visible'], 'Wayland dialog shortcut transfers owner and dialog to XR')
        check(scene()['focused'] not in [owner, dialog], 'Desktop focus no longer targets the transferred window')
        presentation(True)
        wait(lambda: scene()['focused'] == dialog)
        transfer_key()
        wait(lambda: not window(owner)['xr_visible'])
        check(window(owner)['desktop_visible'] and window(dialog)['desktop_visible'] and not window(dialog)['xr_visible'], 'Wayland owner and dialog return exclusively to desktop')
        check(owner not in [p['id'] for p in get('/api/state')['panels']], 'Desktop-only window is absent from XR panels')
        post('/api/xr-input', {'events': [{'kind': 'motion', 'id': owner, 'uv': [.5, .5]}],
             'actions': [{'op': 'focus', 'id': owner}]})
        time.sleep(.3)
        check(scene()['focused'] not in [owner, dialog], 'Stale XR packets cannot focus a desktop-only window')
        presentation(False)
        wait(lambda: scene()['focused'] == dialog)
        check(client.poll() is None and all(window(id)['desktop'] == before[id] for id in before), 'Wayland process, window identities and geometry survive round trip')
        x = display.Display(bridge)
        win = x.screen().root.create_window(100, 100, 360, 180, 0, x.screen().root_depth, X.InputOutput,
                                            X.CopyFromParent, background_pixel=0x335577)
        win.set_wm_class('transfer-fixture', 'TransferFixture')
        win.set_wm_name('Transfer X11 fixture')
        win.map()
        x.sync()
        xid = wait(lambda: next((w['id'] for w in scene()['windows'] if w['title'] == 'Transfer X11 fixture'), None))
        post('/api/action', {'op': 'focus', 'id': xid})
        transfer_key()
        wait(lambda: not window(xid)['desktop_visible'])
        presentation(True)
        wait(lambda: scene()['focused'] == xid)
        transfer_key()
        wait(lambda: not window(xid)['xr_visible'])
        check(window(xid)['desktop_visible'] and win.get_attributes().map_state != X.IsUnmapped, 'X11 window transfers both ways without recreation or unmapping its client')
        presentation(False)
        wait(lambda: scene()['focused'] == xid)
        transfer_key()
        wait(lambda: not window(xid)['desktop_visible'])
        stop.set()
        pulse.join(timeout=3)
        post('/api/xr-session', {'visible': False, 'focused': False,
             'head': {'position_m': [0, 1.4, 0], 'orientation_xyzw': [0, 0, 0, 1]}, 'horizontal_fov_deg': 100})
        wait(lambda: window(xid)['desktop_visible'])
        check(True, 'XR shutdown brings transferred windows back to desktop')
        win.destroy()
        x.sync()
        result = 0
finally:
    stop.set()
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    for log_file in logs:
        log_file.close()
    (out / 'report.json').write_text(json.dumps({'checks': checks, 'passed': result == 0}, indent=2))
sys.exit(result)
