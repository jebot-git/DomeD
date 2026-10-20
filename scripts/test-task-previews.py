#!/usr/bin/env python3
"""Taskbar preview regression: grouped large clients, live pixels and input latency.
Run with uv run --no-project --with PySide6 --with pillow --with python-xlib.
Uses an isolated Xvfb display and D-Bus session; never opens the active seat.
"""
import json
import os
from pathlib import Path
import re
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request

import PySide6
from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest

if os.environ.get('DOME_PREVIEW_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_PREVIEW_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/task-previews'
out.mkdir(parents=True, exist_ok=True)
children, logs = [], []


def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    children.append(child)
    return child


def wait(check, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.01)
    raise AssertionError('Timed out; see artifacts/task-previews')


try:
    with tempfile.TemporaryDirectory(prefix='dome-task-previews-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'

        def scene():
            return json.load(urllib.request.urlopen(base + '/api/state', timeout=2))['scene']

        def action(value):
            request = urllib.request.Request(base + '/api/action', data=json.dumps(value).encode(),
                                             headers={'Content-Type': 'application/json'})
            urllib.request.urlopen(request, timeout=2).close()

        rd, wr = os.pipe()
        xvfb = os.environ.get('DOME_XVFB') or shutil.which('Xvfb')
        assert xvfb, 'Set DOME_XVFB or install Xvfb'
        start([xvfb, '-displayfd', str(wr), '-screen', '0', '1600x1000x24', '-nolisten', 'tcp'],
              'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display_name = ':' + pipe.readline().strip()
        env = {**os.environ, 'DISPLAY': display_name, 'XDG_RUNTIME_DIR': directory,
               'XDG_CONFIG_HOME': str(runtime / 'config'), 'XDG_DATA_HOME': str(runtime / 'data'),
               'DOME_PREVIEW_PORT': str(port), 'DOME_LOGIN_SESSION': '0', 'DOME_START_XR': '0',
               'DOME_RENDERER': os.environ.get('DOME_TEST_RENDERER', 'gles'),
               'LIBGL_ALWAYS_SOFTWARE': '1', 'RUST_LOG': 'warn'}
        for key in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM']:
            env.pop(key, None)
        (runtime / 'config/dome').mkdir(parents=True)
        (runtime / 'config/dome/keyboard.json').write_text(json.dumps({'ime': False, 'layout': 'us'}))
        start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/debug/dome-compositor')),
               '--backend', 'nested'], 'compositor', env)
        wait(lambda: scene()['source'] == 'wayland')
        wayland = re.search(r'Dome Wayland socket: (\S+)', (out / 'compositor.log').read_text())[1]
        client_env = {**env, 'WAYLAND_DISPLAY': wayland, 'QT_QPA_PLATFORM': 'wayland',
                      'QT_WAYLAND_CLIENT_BUFFER_INTEGRATION': 'shm',
                      'LD_LIBRARY_PATH': str(Path(PySide6.__file__).parent / 'Qt/lib')}
        fixture = runtime / 'client.py'
        fixture.write_text('''
import sys
from pathlib import Path
from PySide6.QtCore import QTimer,Qt
from PySide6.QtGui import QPainter,QColor
from PySide6.QtWidgets import QApplication,QWidget
app=QApplication([]);app.setDesktopFileName('preview-regression')
class Window(QWidget):
 def paintEvent(self,event):
  painter=QPainter(self);painter.fillRect(self.rect(),QColor(Path(sys.argv[1]).read_text()))
 def keyPressEvent(self,event):
  with open(sys.argv[2],'a') as log: log.write('key\\n')
w=Window(None,Qt.WindowType.FramelessWindowHint);w.setFocusPolicy(Qt.FocusPolicy.StrongFocus);w.setWindowTitle(sys.argv[3]);w.resize(3840,2160);w.show()
timer=QTimer();timer.timeout.connect(w.update);timer.start(250)
sys.exit(app.exec())
''')
        colour = runtime / 'colour'
        colour.write_text('#36a381')
        journal = runtime / 'keys'
        journal.write_text('')
        for i in range(4):
            start([sys.executable, str(fixture), str(colour), str(journal), f'preview-{i}'],
                  f'client-{i}', client_env)
        wait(lambda: len(scene()['windows']) == 4)
        for window in scene()['windows']:
            action({'op': 'desktop_resize', 'id': window['id'], 'w': 3840, 'h': 2160})
        xd = display.Display(display_name)
        host = wait(lambda: next((w for w in xd.screen().root.query_tree().children
                                 if str(w.get_wm_name() or '').startswith('Dome')), None))

        def motion(x, y):
            geo = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=geo.x + x, y=geo.y + y)
            xd.sync()

        def screenshot():
            geo = host.get_geometry()
            raw = host.get_image(0, 0, geo.width, geo.height, X.ZPixmap, 0xffffffff)
            return Image.frombytes('RGB', (geo.width, geo.height), raw.data, 'raw', 'BGRX')

        def latency():
            samples = []
            code = xd.keysym_to_keycode(XK.string_to_keysym('a'))
            # Wait for host focus and the Wayland keyboard enter to settle before sampling.
            count = journal.read_text().count('key')
            for _ in range(30):
                xtest.fake_input(xd, X.KeyPress, code); xd.sync(); time.sleep(.02)
                xtest.fake_input(xd, X.KeyRelease, code); xd.sync(); time.sleep(.1)
                if journal.read_text().count('key') > count: break
            assert journal.read_text().count('key') > count, 'Wayland client did not receive keyboard focus'
            for _ in range(12):
                count = journal.read_text().count('key')
                began = time.monotonic()
                xtest.fake_input(xd, X.KeyPress, code)
                xd.sync()
                time.sleep(.02)
                xtest.fake_input(xd, X.KeyRelease, code)
                xd.sync()
                wait(lambda: journal.read_text().count('key') > count, timeout=4)
                samples.append(time.monotonic() - began)
            return {'median_ms': statistics.median(samples)*1000, 'max_ms': max(samples)*1000}

        motion(900, 500)
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xtest.fake_input(xd, X.ButtonPress, 1)
        xtest.fake_input(xd, X.ButtonRelease, 1)
        xd.sync()
        time.sleep(.5)
        focused = scene()['windows'][-1]['id']
        action({'op': 'focus', 'id': focused})
        wait(lambda: scene()['focused'] == focused)
        time.sleep(.3)
        (out / 'scene.json').write_text(json.dumps(scene(), indent=2))
        screenshot().save(out / 'baseline.png')
        baseline = latency()
        motion(160, 770)

        def previews_match(target):
            image = screenshot()
            return all(abs(a-b) < 8 for a, b in zip(image.getpixel((106, 377)), (38,45,55))) and all(all(abs(a-b) < 8 for a, b in zip(image.getpixel((170, 420 + row*88)), target))
                       for row in range(4))

        wait(lambda: previews_match((54, 163, 129)))
        screenshot().save(out / 'group.png')
        hovered = latency()
        colour.write_text('#a36add')
        wait(lambda: previews_match((163, 106, 221)))
        screenshot().save(out / 'updated.png')
        # Wide tolerance for software rendering on shared machines; report actual measurements.
        assert hovered['max_ms'] < max(1000, baseline['max_ms'] * 5), (baseline, hovered)
        motion(900, 500)
        wait(lambda: not previews_match((163, 106, 221)))
        report = {'baseline': baseline, 'hover': hovered, 'clients': 4, 'size': [3840, 2160]}
        (out / 'report.json').write_text(json.dumps(report, indent=2))
        print('PASS: grouped previews, live pixel updates, dismissal and responsive keyboard input')
        print(json.dumps(report))
finally:
    for child in reversed(children):
        child.terminate()
    for child in reversed(children):
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
    for log in logs:
        log.close()
