#!/usr/bin/env python3
"""Miro PDF pixels follow the real Dome Appearance UI on a private session.

Run with uv --with python-xlib --with pillow. No live session is changed.
"""
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

from PIL import Image, ImageStat
from Xlib import X, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_MIRO_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_MIRO_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/miro-appearance'
out.mkdir(parents=True, exist_ok=True)
processes, logs, checks = [], [], []
compositor = None
stop = threading.Event()


def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child


def wait(predicate, timeout=40):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(.1)
    raise AssertionError('Timed out; see artifacts/miro-appearance')


def check(ok, label):
    checks.append({'check': label, 'passed': bool(ok)})
    print(('PASS ' if ok else 'FAIL ') + label, flush=True)
    assert ok, label


class RuntimeDirectory(tempfile.TemporaryDirectory):
    def __exit__(self, *args):
        subprocess.run(['fusermount3', '-u', str(Path(self.name) / 'doc')],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return super().__exit__(*args)


try:
    with RuntimeDirectory(prefix='dome-miro-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        home = runtime / 'home'
        config = home / '.config'
        (config / 'dome').mkdir(parents=True)
        appearance = config / 'dome/appearance.json'
        appearance.write_text(json.dumps({'light': False}))
        (config / 'dome/keyboard.json').write_text(json.dumps({'ime': False, 'layout': 'us'}))
        (config / 'miro-pdf').mkdir()
        preferences = config / 'miro-pdf/miro.conf'
        original = 'Set DarkModeUi False\nSet DarkModePdf False\n'
        preferences.write_text(original)
        binary = os.environ.get('DOME_MIRO', str(Path.home() / '.local/bin/miro-pdf'))
        sample = root / 'build/miro-source/assets/coordinate_points.pdf'
        apps = runtime / 'data/applications'
        apps.mkdir(parents=True)
        (apps / 'miro-pdf.desktop').write_text(
            '[Desktop Entry]\nType=Application\nName=Miro PDF\n'
            f'Exec="{binary}" %f\nMimeType=application/pdf;\nStartupWMClass=miro-pdf\n')
        (apps / 'miro-sample.desktop').write_text(
            '[Desktop Entry]\nType=Application\nName=Miro PDF launch fixture\n'
            f'Exec="{binary}" "{sample}"\nStartupWMClass=miro-pdf\n')
        (apps / 'miro-open.desktop').write_text(
            '[Desktop Entry]\nType=Application\nName=Miro PDF open fixture\n'
            f'Exec=/usr/bin/gio open "{sample}"\nStartupWMClass=miro-pdf\n')
        subprocess.run(['update-desktop-database', str(apps)], check=True)
        (config / 'xdg-desktop-portal').mkdir()
        (config / 'xdg-desktop-portal/dome-portals.conf').write_text((root / 'packaging/dome-portals.conf').read_text())
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        rd, wr = os.pipe()
        start([os.environ.get('DOME_XVFB', 'Xvfb'), '-displayfd', str(wr), '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'],
              'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            ds = ':' + pipe.readline().strip()
        env = {**os.environ, 'HOME': str(home), 'DISPLAY': ds, 'XDG_RUNTIME_DIR': directory,
               'XDG_CONFIG_HOME': str(config), 'XDG_DATA_HOME': str(runtime / 'data'),
               'XDG_STATE_HOME': str(runtime / 'state'), 'XDG_CACHE_HOME': str(runtime / 'cache'),
               'XDG_DATA_DIRS': '/usr/local/share:/usr/share', 'XDG_CURRENT_DESKTOP': 'Dome',
               'XDG_SESSION_DESKTOP': 'dome', 'XDG_SESSION_TYPE': 'wayland', 'DOME_LOGIN_SESSION': '0',
               'DOME_APPEARANCE_SERVICE': '1', 'DOME_START_XR': '0', 'DOME_START_IME': '0',
               'DOME_SESSION_RESTORE': '0', 'DOME_BACKEND': 'nested',
               'DOME_PREVIEW_PORT': str(port), 'DOME_RENDERER': 'gles',
               'XDG_DESKTOP_PORTAL_DIR': str(root / 'packaging'), 'LIBGL_ALWAYS_SOFTWARE': '1',
               'NO_AT_BRIDGE': '1', 'GIO_USE_VFS': 'local', 'RUST_LOG': 'info', 'ICED_BACKEND': 'tiny-skia'}
        for key in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM', 'QT_QPA_PLATFORMTHEME', 'GTK_THEME', 'FONTCONFIG_FILE']:
            env.pop(key, None)
        subprocess.run(['dbus-update-activation-environment', 'HOME', 'DISPLAY', 'XDG_RUNTIME_DIR',
                        'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_DATA_DIRS', 'XDG_CURRENT_DESKTOP',
                        'XDG_SESSION_DESKTOP', 'XDG_SESSION_TYPE', 'XDG_DESKTOP_PORTAL_DIR'], env=env, check=True)
        compositor = start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/release/dome-compositor')), '--backend', 'nested'],
                           'compositor', env, start_new_session=True)

        def get(path):
            return json.load(urllib.request.urlopen(base + path, timeout=3))

        wait(lambda: get('/api/state')['scene']['source'] == 'wayland')
        wayland = wait(lambda: next((p for p in runtime.glob('wayland-*') if p.is_socket()), None))
        miro_env = {**env, 'DISPLAY': '', 'WAYLAND_DISPLAY': wayland.name, 'ICED_BACKEND': 'tiny-skia'}
        app = next(a for a in get('/api/apps') if a['name'] == 'Miro PDF launch fixture')
        opener = next(a for a in get('/api/apps') if a['name'] == 'Miro PDF open fixture')

        def launch(name, desktop='Dome', realm='desktop', pdf=False):
            existing = {w['id'] for w in get('/api/state')['scene']['windows']}
            if desktop == 'Dome':
                entry = opener if pdf or realm == 'xr' else app
                request = urllib.request.Request(base + '/api/launch', data=json.dumps({'id': entry['id'], 'realm': realm}).encode(),
                                                 headers={'Content-Type': 'application/json'})
                urllib.request.urlopen(request, timeout=3).close()
            else:
                start([binary, str(sample)], name, {**miro_env, 'XDG_CURRENT_DESKTOP': desktop})
            return wait(lambda: next((w for w in get('/api/state')['scene']['windows']
                                      if w['app_id'] == 'miro-pdf' and w['id'] not in existing), None))

        window = launch('miro')
        check(window['app_id'] == 'miro-pdf', 'Miro maps as a native Wayland application with its desktop ID')
        check(window['origin'] == 'desktop' and window['desktop_visible'], 'Dome desktop launch maps Miro on the desktop display')
        wait(lambda: 'application/pdf=miro-pdf.desktop;' in (config / 'dome-mimeapps.list').read_text())
        check(True, 'Dome recommends the actual installed Miro desktop entry for PDF opening')

        def pixels(window, label=None):
            with urllib.request.urlopen(base + f'/api/xr-frame/{window["id"]}', timeout=3) as response:
                im = Image.frombytes('RGBA', (int(response.headers['X-Dome-Width']), int(response.headers['X-Dome-Height'])), response.read())
            if label:
                im.save(out / (label + '.png'))
            # The centre of the sample page is blank; compare it across modes.
            w, h = im.size
            return sum(ImageStat.Stat(im.crop((w * .4, h * .4, w * .6, h * .6))).mean[:3]) / 3

        wait(lambda: pixels(window) < 100)
        dark = pixels(window, 'dark')
        check(dark < 100, 'Startup dark appearance overrides light Miro preferences for PDF pages')
        xd = display.Display(ds)
        host = wait(lambda: next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')), None))
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()

        def click(x, y):
            g = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=g.x+x, y=g.y+y)
            xd.sync()
            time.sleep(.1)
            for kind in [X.ButtonPress, X.ButtonRelease]:
                xtest.fake_input(xd, kind, 1)
                xd.sync()
                time.sleep(.1)

        click(20, 18)
        click(100, 222)
        click(750, 230)
        wait(lambda: json.loads(appearance.read_text())['light'])
        wait(lambda: pixels(window) > 180)
        light = pixels(window, 'light')
        check(light > dark + 100, 'Appearance light mode updates an already rendered PDF live')
        log = re.sub(r'\x1b\[[0-9;]*m', '', (out / 'compositor.log').read_text())
        check('dark=false' in log and 'dark=true' in log, 'Both interface and PDF receive startup and live portal changes')

        # Only the renderer heartbeat is simulated; Miro is a real Wayland client.
        # Once XR owns input, use its shell pointer to change Appearance.
        def heartbeat():
            request = urllib.request.Request(base + '/api/xr-session',
                                             data=json.dumps({'id': 991, 'visible': True, 'focused': False}).encode(),
                                             headers={'Content-Type': 'application/json'})
            urllib.request.urlopen(request, timeout=3).close()

        def keep_xr_alive():
            while not stop.wait(.2):
                try:
                    heartbeat()
                except OSError:
                    pass

        heartbeat()
        threading.Thread(target=keep_xr_alive, daemon=True).start()
        second = launch('miro-light-startup', realm='xr')
        check(second['origin'] == 'xr' and not second['desktop_visible'], 'Dome XR launch creates an XR window while desktop Miro is already open')
        current = next(w for w in get('/api/state')['scene']['windows'] if w['id'] == window['id'])
        check(current['origin'] == 'desktop' and current['desktop_visible'], 'XR launch preserves the existing desktop Miro window')
        wait(lambda: pixels(second) > 180)
        check(True, 'A subsequent Miro launch inherits the current light appearance')
        third = launch('miro-desktop-after-xr', pdf=True)
        check(third['origin'] == 'desktop' and third['desktop_visible'], 'Opening a desktop PDF while XR Miro runs creates a desktop window')
        wait(lambda: pixels(third) > 180)
        wait(lambda: get('/api/xr-control')['input_realm'] == 'xr')

        def xr_click(x, y):
            layout = get('/api/xr-shell')
            def send(events):
                request = urllib.request.Request(base + '/api/xr-input', data=json.dumps({'events': events, 'actions': []}).encode(),
                                                 headers={'Content-Type': 'application/json'})
                urllib.request.urlopen(request, timeout=3).close()
            send([{'kind': 'motion', 'id': 0, 'uv': [x / layout['width'], y / layout['top']]}])
            time.sleep(.15)
            send([{'kind': 'button', 'button': 272, 'pressed': True},
                  {'kind': 'button', 'button': 272, 'pressed': False}])
            time.sleep(.25)

        xr_click(20, 18)
        xr_click(100, 222)
        xr_click(600, 218)
        wait(lambda: not json.loads(appearance.read_text())['light'])
        wait(lambda: pixels(window) < 100)
        wait(lambda: pixels(second) < 100)
        wait(lambda: pixels(third) < 100)
        pixels(window, 'dark-again')
        check(True, 'Appearance dark mode restores the rendered PDF live')
        outside = launch('miro-outside-dome', 'Other')
        wait(lambda: pixels(outside) > 180)
        check(True, 'Outside Dome, saved light Miro preferences override the dark desktop portal')
        check(preferences.read_text() == original, 'Session appearance leaves saved Miro preferences unchanged')
        (out / 'report.json').write_text(json.dumps({'checks': checks, 'dark': dark, 'light': light}, indent=2))
finally:
    stop.set()
    if compositor is not None:
        try:
            os.killpg(compositor.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
    if compositor is not None:
        try:
            os.killpg(compositor.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    for log in logs:
        log.close()
