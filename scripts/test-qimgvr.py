#!/usr/bin/env python3
"""Private nested DomeD: default image opener -> SBS recognition -> XR-only client.

Runs on Xvfb/private D-Bus and fakes only the renderer heartbeat. It tests actual
Wayland pixels and the native compositor, not headset optics. Requires qimgvr
1.2.0, Arc SBS 0.4.0 and numpy/OpenCV in this interpreter.
"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

import cv2
import numpy as np

if os.environ.get('DOME_QIMGVR_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              dict(os.environ, DOME_QIMGVR_TEST_BUS='1'))

root = Path(__file__).resolve().parents[1]
viewer = Path(os.environ.get('DOME_QIMGVR', str(Path.home() / '.local/opt/qimgvr/bin/qimgvr')))
output = root / 'artifacts/qimgvr'
output.mkdir(parents=True, exist_ok=True)
(output / 'report.json').unlink(missing_ok=True)
children, logs = [], []
stop = threading.Event()
with socket.socket() as sock:
    sock.bind(('127.0.0.1', 0)); port = sock.getsockname()[1]
base = f'http://127.0.0.1:{port}'
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def request(path, value=None):
    req = urllib.request.Request(base + path, data=None if value is None else json.dumps(value).encode(),
                                 headers={'Content-Type': 'application/json'})
    return http.open(req, timeout=2)


def state():
    with request('/api/state') as response: return json.load(response)['scene']


def wait(predicate, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if isinstance(result, np.ndarray) or result: return result
        except (OSError, ValueError, KeyError, StopIteration): pass
        time.sleep(.08)
    raise AssertionError('Integration timed out; see artifacts/qimgvr logs')


def start(args, name, env, **options):
    log = (output / f'{name}.log').open('w'); logs.append(log)
    child = subprocess.Popen(list(map(str, args)), env=env, stdout=log, stderr=subprocess.STDOUT, **options)
    children.append(child)
    return child


try:
    with tempfile.TemporaryDirectory(prefix='dome-qimgvr-') as directory:
        runtime = Path(directory); runtime.chmod(0o700)
        xvfb = shutil.which('Xvfb') or str(root / '.local-deps/root/usr/bin/Xvfb')
        rd, wr = os.pipe()
        start([xvfb, '-displayfd', wr, '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'],
              'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe: display = ':' + pipe.readline().strip()
        env = dict(os.environ, DISPLAY=display, XDG_RUNTIME_DIR=directory, TMPDIR=directory,
                   XDG_CONFIG_HOME=str(runtime / 'config'), XDG_DATA_HOME=str(runtime / 'data'),
                   XDG_STATE_HOME=str(runtime / 'state'), XDG_DATA_DIRS='/usr/local/share:/usr/share',
                   DOME_PREVIEW_PORT=str(port), DOME_START_XR='0', LIBGL_ALWAYS_SOFTWARE='1',
                   RUST_LOG='error', XR_RUNTIME_JSON=str(runtime / 'no-runtime.json'))
        for key in ('WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM'): env.pop(key, None)
        compositor = start([root / 'target/debug/dome-compositor', '--backend', 'nested'], 'compositor', env)
        wait(state)
        records = list(runtime.glob('*.dome-session.json'))
        desktop = next(p for p in records if json.loads(p.read_text())['realm'] == 'desktop')
        xr = next(p for p in records if json.loads(p.read_text())['realm'] == 'xr')
        desktop_socket = str(desktop).removesuffix('.dome-session.json')
        xr_socket = str(xr).removesuffix('.dome-session.json')
        assert json.loads(desktop.read_text())['control_port'] == port
        install = Path.home() / 'qimgvr/scripts/install-domed.py'
        subprocess.run([sys.executable, str(install), '--executable', str(viewer)], env=env, check=True)
        viewer_env = dict(env, WAYLAND_DISPLAY=desktop_socket, QT_QPA_PLATFORM='wayland', XDG_CURRENT_DESKTOP='Dome')
        defaults = subprocess.check_output(['gio', 'mime', 'image/png'], env=viewer_env, text=True)
        assert 'qimgvr-domed.desktop' in defaults, defaults

        def heartbeat():
            while not stop.wait(.2):
                try:
                    with request('/api/xr-session', {'id': 991, 'visible': True, 'focused': True}): pass
                except OSError: pass
        thread = threading.Thread(target=heartbeat, daemon=True); thread.start()
        wait(lambda: json.load(request('/api/integrations/qimgvr'))['xr_active'])
        image = np.zeros((100, 320, 3), np.uint8)
        image[:, :160] = [0, 0, 255]; image[:, 160:] = [255, 0, 0]
        source = runtime / 'pair #ü_SBS.png'
        cv2.imwrite(str(source), image)
        # A missing model proves the automatic opener bypasses inference for SBS.
        viewer_env['ARC_SBS_MODEL'] = str(runtime / 'deliberately-missing.xml')
        subprocess.run(['gio', 'open', source.as_uri()], env=viewer_env, check=True)
        stereo = wait(lambda: next(w for w in state()['windows'] if w['app_id'] == 'org.qimgvr.Sbs'))
        main = next(w for w in state()['windows'] if w['app_id'] == 'qimgvr')
        assert main['origin'] == 'desktop'
        assert stereo['origin'] == 'xr' and stereo['desktop_visible'] is False
        assert stereo['stereo'] == 'side_by_side'
        def pixels():
            with request(f'/api/xr-frame/{stereo["id"]}') as response:
                width, height = int(response.headers['X-Dome-Width']), int(response.headers['X-Dome-Height'])
                return np.frombuffer(response.read(), np.uint8).reshape(height, width, 4)
        rgba = wait(pixels)
        height, width = rgba.shape[:2]
        left, right = rgba[height // 2, width // 4, :3], rgba[height // 2, 3 * width // 4, :3]
        assert left[0] > 230 and left[2] < 20, left
        assert right[2] > 230 and right[0] < 20, right
        cv2.imwrite(str(output / 'xr-sbs-surface.png'), cv2.cvtColor(rgba, cv2.COLOR_RGBA2BGRA))
        with request('/api/action', {'op': 'close', 'id': main['id']}): pass
        wait(lambda: not state()['windows'])
        # Ordinary 2D input still uses the installed A770 depth/rendering worker.
        viewer_env['ARC_SBS_MODEL'] = os.environ.get('DOME_QIMGVR_MODEL',
            str(Path.home() / '.local/share/arc-sbs/models/depth-small.xml'))
        ordinary = runtime / 'ordinary.png'
        cv2.imwrite(str(ordinary), image)
        subprocess.run(['gio', 'open', ordinary.as_uri()], env=viewer_env, check=True)
        converted = wait(lambda: next(w for w in state()['windows'] if w['app_id'] == 'org.qimgvr.Sbs'))
        assert converted['origin'] == 'xr'
        def converted_file():
            for file in runtime.glob('qimgvr-*/display.png'):
                content = cv2.imread(str(file))
                if content is not None and content.shape[1] == 640: return file
        assert wait(converted_file)
        main = next(w for w in state()['windows'] if w['app_id'] == 'qimgvr')
        with request('/api/action', {'op': 'close', 'id': main['id']}): pass
        wait(lambda: not state()['windows'])
        # Straight-alpha PNG must reach the XR compositor as premultiplied RGBA.
        alpha_checks = []
        for preexisting in (True, False):
            alpha = np.zeros((160, 320 if preexisting else 160, 4), np.uint8)
            alpha[40:120, 40:120] = [0, 0, 255, 128]
            if preexisting: alpha[40:120, 200:280] = [255, 0, 0, 64]
            transparent = runtime / ('alpha_SBS.png' if preexisting else 'alpha_2d.png')
            cv2.imwrite(str(transparent), alpha)
            viewer_env['ARC_SBS_MODEL'] = (str(runtime / 'missing.xml') if preexisting else
                os.environ.get('DOME_QIMGVR_MODEL', str(Path.home() / '.local/share/arc-sbs/models/depth-small.xml')))
            subprocess.run(['gio', 'open', transparent.as_uri()], env=viewer_env, check=True)
            alpha_window = wait(lambda: next(w for w in state()['windows'] if w['app_id'] == 'org.qimgvr.Sbs'))
            def alpha_pixels():
                with request(f'/api/xr-frame/{alpha_window["id"]}') as response:
                    aw, ah = int(response.headers['X-Dome-Width']), int(response.headers['X-Dome-Height'])
                    frame = np.frombuffer(response.read(), np.uint8).reshape(ah, aw, 4)
                    return frame if frame[:, :, :3].any() else None
            rgba_alpha = wait(alpha_pixels)
            ah, aw = rgba_alpha.shape[:2]
            al, ar = rgba_alpha[ah//2, aw//4], rgba_alpha[ah//2, 3*aw//4]
            np.testing.assert_allclose(al, [128, 0, 0, 128], atol=1)
            np.testing.assert_allclose(ar, [0, 0, 64, 64] if preexisting else [128, 0, 0, 128], atol=1)
            assert rgba_alpha[0, 0, 3] == 0 and rgba_alpha[ah//2, 0, 3] == 0
            # The intermediate PNG is straight alpha, not flattened/premultiplied storage.
            displays = list(runtime.glob('qimgvr-*/display.png'))
            assert len(displays) == 1
            stored = cv2.imread(str(displays[0]), cv2.IMREAD_UNCHANGED)
            assert stored.shape[2] == 4 and stored[:, :, 3].min() == 0
            assert stored[:, :, 2].max() == 255
            alpha_checks.append({'existing_sbs': preexisting, 'left_rgba': al.tolist(), 'right_rgba': ar.tolist()})
            cv2.imwrite(str(output / ('alpha-sbs.png' if preexisting else 'alpha-2d.png')),
                        cv2.cvtColor(rgba_alpha, cv2.COLOR_RGBA2BGRA))
            main = next(w for w in state()['windows'] if w['app_id'] == 'qimgvr')
            with request('/api/action', {'op': 'close', 'id': main['id']}): pass
            wait(lambda: not state()['windows'])
        # Inactive DomeD does not send images to WayVR even if WiVRn is running.
        stop.set(); thread.join(timeout=3)
        time.sleep(2.1)
        subprocess.run(['gio', 'open', source.as_uri()], env=viewer_env, check=True)
        wait(lambda: len(state()['windows']) == 1)
        time.sleep(.8)
        assert not any(w['app_id'] == 'org.qimgvr.Sbs' for w in state()['windows'])
        report = {'default_opener': True, 'file_url_unicode': True, 'existing_sbs_no_model': True,
                  'main_realm': 'desktop', 'presenter_realm': 'xr', 'stereo': 'side_by_side',
                  'left_rgba': left.tolist(), 'right_rgba': right.tolist(), 'cleanup': True,
                  'ordinary_2d_conversion': True,
                  'alpha_checks': alpha_checks,
                  'inactive_session_no_xr_window': True, 'headset_verified': False}
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report), flush=True)
        for window in state()['windows']:
            with request('/api/action', {'op': 'close', 'id': window['id']}): pass
        wait(lambda: not state()['windows'])
finally:
    stop.set()
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
            try: child.wait(timeout=5)
            except subprocess.TimeoutExpired: child.kill(); child.wait(timeout=5)
    for log in logs: log.close()
