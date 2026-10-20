#!/usr/bin/env python3
"""Real keyboard navigation and PATH launching on a private nested Dome seat.

Run with uv --no-project --with python-xlib --with pillow python scripts/test-launcher.py.
Requires Xvfb and a compositor built with nested support. No host defaults change.
"""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_LAUNCHER_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_LAUNCHER_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/launcher'
out.mkdir(parents=True, exist_ok=True)
processes, logs, checks = [], [], []

def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child

def wait(check, timeout=12):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.04)
    raise AssertionError('Timed out; see artifacts/launcher')

try:
    with tempfile.TemporaryDirectory(prefix='dome-launcher-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        apps = runtime / 'data/applications'
        apps.mkdir(parents=True)
        binaries = runtime / 'bin'
        binaries.mkdir()
        (runtime / 'empty').mkdir()
        marker = runtime / 'launched.jsonl'
        probe = binaries / 'dome-launch-probe'
        probe.write_text('#!/usr/bin/python3\nimport json,os,sys\n'
                         'with open(os.environ["DOME_LAUNCHER_MARKER"],"a") as output:\n'
                         ' output.write(json.dumps({"args":sys.argv[1:],"socket":os.environ.get("WAYLAND_DISPLAY"),'
                         '"inherited_socket":os.environ.get("WAYLAND_SOCKET")})+"\\n")\n')
        probe.chmod(0o755)
        path_app = binaries / 'domepathapp'
        path_app.write_text('#!/bin/sh\nexec "' + str(probe) + '" PATH\n')
        path_app.chmod(0o755)
        (binaries / 'domenonexec').write_text('not executable\n')
        for index in range(25):
            (apps / f'row{index:02}.desktop').write_text(
                f'[Desktop Entry]\nType=Application\nName=Launch Row{index:02}\n'
                f'Exec={probe} row{index:02}\nComment=' + ('domepathapp' if index == 0 else '') + '\n')
        rd, wr = os.pipe()
        start([os.environ.get('DOME_XVFB', 'Xvfb'), '-displayfd', str(wr), '-screen', '0', '1280x800x24',
               '-nolisten', 'tcp'], 'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display_name = ':' + pipe.readline().strip()
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        env = {**os.environ, 'DISPLAY': display_name, 'XDG_RUNTIME_DIR': directory,
               'XDG_DATA_HOME': str(runtime / 'data'), 'XDG_DATA_DIRS': str(runtime / 'empty'),
               'XDG_CONFIG_HOME': str(runtime / 'config'), 'XDG_STATE_HOME': str(runtime / 'state'),
               'XDG_CURRENT_DESKTOP': 'Dome', 'PATH': str(binaries) + os.pathsep + os.environ['PATH'],
               'DOME_LAUNCHER_MARKER': str(marker), 'DOME_LOGIN_SESSION': '0', 'DOME_START_XR': '0',
               'DOME_START_IME': '0', 'DOME_RENDERER': 'gles', 'DOME_PREVIEW_PORT': str(port),
               'LIBGL_ALWAYS_SOFTWARE': '1', 'RUST_LOG': 'warn'}
        for key in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM', 'DOME_APPEARANCE_SERVICE']:
            env.pop(key, None)
        start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/debug/dome-compositor')),
               '--backend', 'nested'], 'compositor', env)
        def get(path):
            return json.load(urllib.request.urlopen(f'http://127.0.0.1:{port}' + path, timeout=2))
        wait(lambda: get('/api/state')['scene']['source'] == 'wayland')
        assert len(get('/api/apps')) == 25
        xd = display.Display(display_name)
        host = wait(lambda: next((w for w in xd.screen().root.query_tree().children
                                 if str(w.get_wm_name() or '').startswith('Dome')), None))
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        def key(name, down=None):
            code = xd.keysym_to_keycode(XK.string_to_keysym(name))
            assert code, name
            for press in ([True, False] if down is None else [down]):
                xtest.fake_input(xd, X.KeyPress if press else X.KeyRelease, code)
                xd.sync()
                time.sleep(.025)
        def text(value):
            for character in value:
                key(character)
        def click(x, y):
            geometry = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=geometry.x+x, y=geometry.y+y)
            xd.sync()
            for kind in [X.ButtonPress, X.ButtonRelease]:
                xtest.fake_input(xd, kind, 1)
                xd.sync()
                time.sleep(.06)
        def open_menu():
            click(40, host.get_geometry().height - 30)
        def records():
            return [json.loads(line) for line in marker.read_text().splitlines()] if marker.exists() else []
        def launch(expected=None, press_return=True):
            before = len(records())
            if press_return:
                key('Return')
            value = wait(lambda: records()[before:] if len(records()) > before else None)[0]
            assert value['socket'] and value['inherited_socket'] is None, value
            if expected is not None:
                assert value['args'] == [expected], value
            return value['args'][0]

        open_menu(); key('End'); launch('row24')
        checks.append('End reaches entries beyond the first visible page')
        open_menu(); key('End'); key('Down'); launch('row00')
        open_menu(); key('Home'); key('Up'); launch('row24')
        checks.append('Cursor navigation wraps from either end of the catalog')
        open_menu(); key('Down', True); time.sleep(.95); key('Down', False)
        geometry = host.get_geometry()
        raw = host.get_image(0, 0, geometry.width, geometry.height, X.ZPixmap, 0xffffffff)
        Image.frombytes('RGB', (geometry.width, geometry.height), raw.data, 'raw', 'BGRX').save(out / 'scrolled.png')
        selected = launch()
        assert 8 <= int(selected.removeprefix('row')) < 25, selected
        checks.append('Holding Down repeats and scrolls beyond the first eight rows')
        open_menu(); key('End'); text('row02'); launch('row02')
        checks.append('Typing after navigation filters the whole catalog and resets selection')
        open_menu(); key('End')
        before = len(records()); click(100, 120)
        wait(lambda: len(records()) > before)
        assert records()[-1]['args'] == ['row17'], records()[-1]
        checks.append('Mouse selection addresses the scrolled rows')
        open_menu(); text('domepathapp'); launch('PATH')
        checks.append('Exact PATH executable runs without a desktop entry, ahead of a partial menu match')
        open_menu(); text('domepathapp'); key('Down'); launch('row00')
        checks.append('Explicit cursor selection retains desktop-entry launch semantics')
        open_menu(); text('domenonexec'); before = len(records()); key('Return'); time.sleep(.25)
        assert len(records()) == before
        key('Escape')
        checks.append('Non-executable PATH files are rejected without launching another app')
        open_menu(); key('Down', True); key('Escape'); open_menu(); time.sleep(.65)
        launch('row00'); key('Down', False)
        checks.append('Closing and reopening cancels a held cursor repeat')
        (out / 'report.json').write_text(json.dumps({'checks': checks, 'launches': records()}, indent=2))
        print('\n'.join('PASS: ' + check for check in checks))
finally:
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
    for child in reversed(processes):
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    for log in logs:
        log.close()
