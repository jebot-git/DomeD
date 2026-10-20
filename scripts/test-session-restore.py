#!/usr/bin/env python3
"""Two real logins on private Wayland sockets, including a late XR presenter.

Run with uv --no-project --with PySide6 --with python-xlib --with pillow python scripts/test-session-restore.py.
Only the XR renderer heartbeat is simulated; clients and saved launch commands are real.
Use the installed full build or build with --features nested,xr,standalone.
"""
import json
import math
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if '--fixture' in sys.argv:
    from PySide6.QtWidgets import QApplication, QLabel
    assert Path(sys.argv[-1]).is_file(), 'Saved working directory or document argument was lost'
    assert sys.argv[-2] == '', 'Empty application argument was lost'
    application = QApplication(sys.argv)
    application.setDesktopFileName(sys.argv[sys.argv.index('--fixture') + 1])
    window = QLabel('Session restore fixture · ' + sys.argv[-1])
    colour = '#b06040' if application.desktopFileName() == 'session.background' else '#406080'
    window.setStyleSheet('background: ' + colour + '; color: white')
    window.setWindowTitle('Saved ' + application.desktopFileName())
    window.resize(420, 240)
    window.show()
    raise SystemExit(application.exec())
if os.environ.get('DOME_RESTORE_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_RESTORE_TEST_BUS': '1'})

root = Path(__file__).resolve().parents[1]
from Xlib import X, display
from PIL import Image
out = root / 'artifacts/session-restore'
out.mkdir(parents=True, exist_ok=True)
processes, logs, checks = [], [], []
stop = threading.Event()
heartbeat = None

def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child

def wait(check, timeout=15):
    until = time.monotonic() + timeout
    last = None
    while time.monotonic() < until:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, KeyError, StopIteration) as error:
            last = error
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/session-restore: ' + str(last))

def terminate(child):
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=8)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()

def same(actual, expected):
    if isinstance(expected, float):
        return isinstance(actual, (int, float)) and math.isclose(actual, expected, rel_tol=1e-6, abs_tol=1e-6)
    if isinstance(expected, dict):
        return actual.keys() == expected.keys() and all(same(actual[key], value) for key, value in expected.items())
    if isinstance(expected, list):
        return len(actual) == len(expected) and all(same(a, b) for a, b in zip(actual, expected))
    return actual == expected

try:
    with tempfile.TemporaryDirectory(prefix='dome-restore-') as directory:
        temporary = Path(directory)
        rd, wr = os.pipe()
        start([os.environ.get('DOME_XVFB', 'Xvfb'), '-displayfd', str(wr), '-screen', '0', '1280x800x24',
               '-nolisten', 'tcp'], 'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display_name = ':' + pipe.readline().strip()
        xd = display.Display(display_name)
        def editor_on_top():
            host = next((w for w in xd.screen().root.query_tree().children
                         if str(w.get_wm_name() or '').startswith('Dome')), None)
            if host is None:
                return False
            raw = host.get_image(500, 200, 1, 1, X.ZPixmap, 0xffffffff)
            colour = Image.frombytes('RGB', (1, 1), raw.data, 'raw', 'BGRX').getpixel((0, 0))
            editor_on_top.last_colour = colour
            return all(abs(a-b) < 4 for a,b in zip(colour, (64, 96, 128)))
        saved_path = temporary / 'state/dome/session.json'
        base_env = {**os.environ, 'DISPLAY': display_name, 'XDG_STATE_HOME': str(temporary / 'state'),
                    'XDG_CONFIG_HOME': str(temporary / 'config'), 'XDG_DATA_HOME': str(temporary / 'data'),
                    'DOME_LOGIN_SESSION': '0', 'DOME_SESSION_RESTORE': '1', 'DOME_START_IME': '0',
                    'DOME_START_XR': '0', 'DOME_RENDERER': 'gles', 'LIBGL_ALWAYS_SOFTWARE': '1',
                    'RUST_LOG': 'warn'}
        for name in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM', 'DOME_APPEARANCE_SERVICE']:
            base_env.pop(name, None)
        def login(number, restore=True):
            runtime = temporary / ('runtime' + str(number))
            runtime.mkdir(mode=0o700)
            with socket.socket() as sock:
                sock.bind(('127.0.0.1', 0))
                port = sock.getsockname()[1]
            env = {**base_env, 'XDG_RUNTIME_DIR': str(runtime), 'DOME_PREVIEW_PORT': str(port),
                   'DOME_SESSION_RESTORE': '1' if restore else '0'}
            compositor = start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/debug/dome-compositor')),
                                '--backend', 'nested'], 'login' + str(number), env)
            base = f'http://127.0.0.1:{port}'
            def request(path, value=None):
                req = urllib.request.Request(base + path, data=None if value is None else json.dumps(value).encode(),
                                             headers={'Content-Type': 'application/json'})
                return urllib.request.urlopen(req, timeout=2)
            def scene():
                return json.load(request('/api/state'))['scene']
            wait(lambda: scene()['source'] == 'wayland')
            contexts = wait(lambda: list(runtime.glob('*.dome-session.json')))
            sockets = {json.loads(path.read_text())['realm']: str(path).removesuffix('.dome-session.json') for path in contexts}
            return compositor, env, request, scene, sockets
        document = temporary / 'document #ü with spaces.txt'
        document.write_text('A path whose argument boundaries must survive login.\n')
        first, env, request, scene, sockets = login(1)
        stop.clear()
        def pulse():
            while not stop.wait(.15):
                try:
                    request('/api/xr-session', {'id': 887, 'visible': True, 'focused': False}).close()
                except OSError:
                    pass
        heartbeat = threading.Thread(target=pulse, daemon=True)
        heartbeat.start()
        wait(lambda: json.load(request('/api/integrations/qimgvr'))['xr_active'])
        clients = []
        for app_id, realm in [('session.editor', 'desktop'), ('session.xr', 'xr'), ('org.qimgvr.Sbs', 'xr'), ('session.background', 'desktop')]:
            clients.append(start([sys.executable, str(Path(__file__).resolve()), '--fixture', app_id, '', document.name],
                                 app_id, {**env, 'WAYLAND_DISPLAY': sockets[realm], 'QT_QPA_PLATFORM': 'wayland'}, cwd=temporary))
        wait(lambda: len(scene()['windows']) == 4)
        def windows():
            return {window['app_id']: window for window in scene()['windows']}
        wait(lambda: len(windows()) == 4 and 'session.editor' in windows() and 'org.qimgvr.Sbs' in windows())
        ids = {app_id: window['id'] for app_id, window in windows().items()}
        def action(**value):
            request('/api/action', value).close()
        action(op='add_workspace', name='Restored Editing')
        workspace = len(scene()['workspaces']) - 1
        action(op='move', id=ids['session.editor'], workspace=workspace, lane=2)
        action(op='desktop_move', id=ids['session.editor'], x=315, y=125)
        action(op='desktop_resize', id=ids['session.editor'], w=610, h=340)
        action(op='maximize', id=ids['session.editor'])
        action(op='move', id=ids['session.background'], workspace=workspace, lane=2)
        action(op='desktop_move', id=ids['session.background'], x=330, y=140)
        action(op='desktop_resize', id=ids['session.background'], w=610, h=340)
        for app_id, scale, x in [('session.xr', 1.4, .4), ('org.qimgvr.Sbs', 1.7, -.6)]:
            action(op='spatial_transform', id=ids[app_id],
                   pose={'position_m': [x, 1.25, -1.8], 'orientation_xyzw': [0, 0, 0, 1]}, scale=scale)
            action(op='dome_offset', id=ids[app_id], yaw_deg=12, pitch_deg=-8)
        action(op='switch_workspace', workspace=workspace)
        action(op='focus', id=ids['session.editor'])
        wait(editor_on_top)
        before = scene()
        stop.set(); heartbeat.join(timeout=2)
        terminate(first)
        for child in clients:
            terminate(child)
        saved = json.loads(saved_path.read_text())
        (out / 'saved-session.json').write_text(json.dumps(saved, indent=2))
        assert saved['version'] == 1 and len(saved['layout']['windows']) == 4, saved
        assert saved_path.stat().st_mode & 0o777 == 0o600
        assert len(saved['restarts']) == 3, saved['restarts']
        assert all(restart['argv'][-1] == document.name and restart['argv'][-2] == ''
                   and restart['cwd'] == str(temporary) for restart in saved['restarts'])
        checks.append('Logout atomically saves private restart arguments, workspaces and desktop/XR placement')
        second, env2, request, scene, sockets2 = login(2)
        wait(lambda: len(scene()['windows']) == 3)
        original = {w['app_id']: w for w in saved['layout']['windows']}
        fields = ['workspace', 'lane', 'desktop', 'restore', 'minimized', 'maximized', 'origin',
                  'desktop_visible', 'xr_pose', 'xr_scale', 'dome_offset_deg', 'pullout_slot', 'pullout_restore_scale']
        def restored():
            current = {w['app_id']: w for w in scene()['windows']}
            return current if set(current) == {'session.editor', 'session.xr', 'session.background'} and all(
                same(current[app_id][field], original[app_id][field]) for app_id in current for field in fields) else None
        try:
            current = wait(restored)
        except AssertionError:
            current = {w['app_id']: w for w in scene()['windows']}
            mismatch = {app: {field: {'expected': original[app][field], 'actual': current[app][field]}
                        for field in fields if not same(current[app][field], original[app][field])}
                        for app in current if app in original}
            (out / 'mismatch.json').write_text(json.dumps(mismatch, indent=2))
            print(json.dumps(mismatch), flush=True)
            raise
        for app_id in current:
            for field in fields:
                assert same(current[app_id][field], original[app_id][field]), (app_id, field, current[app_id], original[app_id])
        assert scene()['active_workspace'] == workspace
        assert [w['name'] for w in scene()['workspaces']] == [w['name'] for w in before['workspaces']]
        try:
            wait(editor_on_top)
        except AssertionError:
            (out / 'stacking-mismatch.json').write_text(json.dumps({'scene': scene(),
                'pixel': getattr(editor_on_top, 'last_colour', None), 'saved': saved}, indent=2))
            raise
        wait(lambda: scene()['focused'] == current['session.editor']['id'])
        checks.append('Desktop stacking order survives application startup order')
        checks.append('Login reopens applications on fresh realm-specific sockets and restores exact desktop/XR layout')
        # The renderer/headset may become available long after ordinary apps.
        time.sleep(31)
        late_saved = json.loads(saved_path.read_text())
        assert any(w['app_id'] == 'org.qimgvr.Sbs' for w in late_saved['layout']['windows'])
        late = start([sys.executable, str(Path(__file__).resolve()), '--fixture', 'org.qimgvr.Sbs', '', document.name],
                     'late-xr-presenter', {**env2, 'WAYLAND_DISPLAY': sockets2['xr'], 'QT_QPA_PLATFORM': 'wayland'}, cwd=temporary)
        wait(lambda: any(w['app_id'] == 'org.qimgvr.Sbs' and same(w['xr_scale'], 1.7) for w in scene()['windows']))
        late_window = next(w for w in scene()['windows'] if w['app_id'] == 'org.qimgvr.Sbs')
        for field in fields:
            assert same(late_window[field], original['org.qimgvr.Sbs'][field]), (field, late_window)
        checks.append('Late headset-dependent XR windows retain saved poses after startup and autosave')
        terminate(second)
        terminate(late)
        persisted = saved_path.read_bytes()
        third, _, _, scene, _ = login(3, restore=False)
        time.sleep(.5)
        assert scene()['windows'] == []
        terminate(third)
        assert saved_path.read_bytes() == persisted
        checks.append('Disabling session restore leaves the saved login session untouched')
        (out / 'report.json').write_text(json.dumps({'checks': checks, 'saved': saved}, indent=2))
        print('\n'.join('PASS: ' + check for check in checks))
finally:
    stop.set()
    if heartbeat:
        heartbeat.join(timeout=2)
    for child in reversed(processes):
        terminate(child)
    for log in logs:
        log.close()
