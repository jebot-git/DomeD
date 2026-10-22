#!/usr/bin/env python3
"""Cursor authority and live Appearance regression on a disposable Xvfb display.

Build with --features nested first. Requires Xvfb, a C compiler, wayland-scanner,
wayland-client development files and wayland-protocols.
Run: uv run --no-project --with python-xlib --with pillow python scripts/test-cursor.py
Set DOME_COMPOSITOR and DOME_RENDERER to choose the binary and renderer.
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
import time
import urllib.request

from PIL import Image
from Xlib import X, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_CURSOR_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_CURSOR_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
renderer = os.environ.get('DOME_RENDERER', 'gles')
out = root / 'artifacts' / ('cursor-' + renderer)
out.mkdir(parents=True, exist_ok=True)
(out / 'report.json').unlink(missing_ok=True)
(out / 'failure.png').unlink(missing_ok=True)
processes, logs, checks = [], [], []
xd = None


def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, env=env, cwd=root, stdout=log,
                             stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child


def wait(fn, timeout=25):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see ' + str(out))


def check(ok, name):
    checks.append({'passed': bool(ok), 'check': name})
    print(('PASS ' if ok else 'FAIL ') + name, flush=True)
    assert ok, name


def get(path):
    with urllib.request.urlopen(base + path, timeout=2) as response:
        return json.load(response)


def motion(x, y):
    geometry = host.get_geometry()
    xtest.fake_input(xd, X.MotionNotify, x=int(geometry.x + x), y=int(geometry.y + y))
    xd.sync()
    time.sleep(.15)


def click(x, y):
    motion(x, y)
    for event in (X.ButtonPress, X.ButtonRelease):
        xtest.fake_input(xd, event, 1)
        xd.sync()
        time.sleep(.05)


def image():
    geometry = host.get_geometry()
    raw = host.get_image(0, 0, geometry.width, geometry.height, X.ZPixmap, 0xffffffff)
    return Image.frombytes('RGB', (geometry.width, geometry.height), raw.data, 'raw', 'BGRX')


def arrow(x, y, light, name):
    fill, edge = ((20, 20, 20), (245, 245, 245)) if light else ((245, 245, 245), (20, 20, 20))

    def matches():
        shot = image()
        return all(shot.getpixel((int(x + dx), int(y + dy))) == color
                   for dx, dy, color in [(2, 6, fill), (0, 6, edge), (4, 12, fill), (8, 12, edge)])

    check(wait(matches), name)
    shot = image()
    crop = shot.crop((x - 25, y - 25, x + 50, y + 50))
    colors = {color for _, color in crop.getcolors(crop.width * crop.height)}
    check(not ({(255, 0, 255), (255, 255, 0)} & colors), name + ': no client cursor pixels')
    shot.save(out / (name.replace(' ', '-') + '.png'))


def probe_log():
    return (out / 'client.log').read_text()


try:
    with tempfile.TemporaryDirectory(prefix='dome-cursor-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        protocol_dir = subprocess.check_output(['pkg-config', '--variable=pkgdatadir', 'wayland-protocols'], text=True).strip()
        xml = protocol_dir + '/stable/xdg-shell/xdg-shell.xml'
        for kind, target in [('client-header', 'xdg-shell-client-protocol.h'), ('private-code', 'xdg-shell-protocol.c')]:
            subprocess.run(['wayland-scanner', kind, xml, str(runtime / target)], check=True)
        import shlex
        flags = shlex.split(subprocess.check_output(['pkg-config', '--cflags', '--libs', 'wayland-client'], text=True))
        subprocess.run(['cc', '-I' + directory, str(root / 'scripts/cursor-client.c'),
                        str(runtime / 'xdg-shell-protocol.c'), '-o', str(runtime / 'client'), *flags], check=True)
        rd, wr = os.pipe()
        start(['Xvfb', '-displayfd', str(wr), '-screen', '0', '1600x1000x24',
               '-nolisten', 'tcp', '-noreset'], 'xvfb', os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display_name = ':' + pipe.readline().strip()
        assert display_name != ':'
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        base = f'http://127.0.0.1:{port}'
        config = runtime / 'config/dome'
        config.mkdir(parents=True)
        (config / 'keyboard.json').write_text(json.dumps({'ime': False, 'layout': 'us'}))
        (config / 'appearance.json').write_text(json.dumps({'light': False}))
        (runtime / 'empty').mkdir()
        env = {**os.environ, 'DISPLAY': display_name, 'XDG_RUNTIME_DIR': directory,
               'XDG_CONFIG_HOME': str(runtime / 'config'), 'XDG_DATA_HOME': str(runtime / 'empty'),
               'XDG_DATA_DIRS': str(runtime / 'empty'), 'XDG_STATE_HOME': str(runtime / 'state'),
               'DOME_LOGIN_SESSION': '0', 'DOME_START_XR': '0', 'DOME_SESSION_RESTORE': '0',
               'XDG_SESSION_TYPE': 'x11', 'DOME_PREVIEW_PORT': str(port), 'DOME_RENDERER': renderer,
               'LIBGL_ALWAYS_SOFTWARE': '1', 'RUST_LOG': 'warn'}
        for name in ('WAYLAND_DISPLAY', 'WAYLAND_SOCKET'):
            env.pop(name, None)
        if renderer == 'vulkan':
            env['VK_DRIVER_FILES'] = os.environ.get('DOME_TEST_VULKAN_ICD', '/usr/share/vulkan/icd.d/lvp_icd.json')
            env['LIBGL_KOPPER_DISABLE'] = 'true'
        compositor = start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/debug/dome-compositor')),
                            '--backend', 'nested'], 'compositor', env)
        wait(lambda: get('/api/state')['scene']['source'] == 'wayland')
        match = re.search(r'Dome Wayland socket: (\S+)', (out / 'compositor.log').read_text())
        assert match
        xd = display.Display(display_name)

        def host_window():
            return next((w for w in xd.screen().root.query_tree().children
                         if str(w.get_wm_name() or '').startswith('Dome')), None)

        host = wait(host_window)
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        motion(950, 500)
        arrow(950, 500, False, 'Dark theme shell cursor')
        xd.xfixes_query_version()
        host_cursor = xd.xfixes_get_cursor_image(host)
        check(all(pixel >> 24 == 0 for pixel in host_cursor.cursor_image), 'Nested host cursor is invisible')
        client = start([str(runtime / 'client')], 'client', {**env, 'WAYLAND_DISPLAY': match[1]})
        window = wait(lambda: next((w for w in get('/api/state')['scene']['windows']
                                   if w['app_id'] == 'dome-cursor-probe'), None))
        rect = window['desktop']
        x, y = int(rect['x']), int(rect['y']) + 90
        wait(lambda: image().getpixel((x + 50, y + 20)) == (64, 128, 96))
        for offset, mode, name in [(30, 0, 'Custom image rejected'), (110, 1, 'Hide request rejected'),
                                    (190, 2, 'Client hotspot rejected'), (270, 3, 'Animated cursor rejected')]:
            wait(lambda: motion(x + offset, y) or f'mode {mode}\n' in probe_log())
            arrow(x + offset, y, False, name)
        motion(x + 280, y)
        wait(lambda: 'commit 1\n' in probe_log())
        arrow(x + 280, y, False, 'Cursor surface update rejected')
        click(x + 280, y)
        check(wait(lambda: 'button hide\n' in probe_log()), 'Client still receives pointer buttons')
        arrow(x + 280, y, False, 'Click hide request rejected')
        motion(950, 500)
        check(wait(lambda: 'leave\n' in probe_log()), 'Client receives pointer leave')
        arrow(950, 500, False, 'Cursor survives focus leave')
        motion(x + 30, y)
        check(wait(lambda: probe_log().count('enter\n') >= 2), 'Client receives pointer reentry')
        arrow(x + 30, y, False, 'Cursor survives focus reentry')
        # Use DomeD's real Appearance controls so buffer invalidation is exercised.
        click(20, 18)
        click(100, 222)
        click(800, (host.get_geometry().height - 500) // 2 + 80)
        check(wait(lambda: json.loads((config / 'appearance.json').read_text())['light']), 'Appearance switches live to light mode')
        arrow(800, 230, True, 'Light theme modal cursor')
        check(wait(lambda: get('/api/xr-control')['light']), 'Appearance propagates to XR cursor control')
        click(640, (host.get_geometry().height - 500) // 2 + 470)
        motion(x + 110, y)
        arrow(x + 110, y, True, 'Light theme client cursor')
        click(20, 18)
        click(100, 222)
        click(800, (host.get_geometry().height - 500) // 2 + 80)
        check(wait(lambda: not json.loads((config / 'appearance.json').read_text())['light']), 'Appearance switches live back to dark mode')
        arrow(800, 230, False, 'Dark theme restored cursor')
        check(wait(lambda: not get('/api/xr-control')['light']), 'XR cursor control restores dark mode')
        client.terminate()
        client.wait(timeout=4)
        motion(950, 500)
        arrow(950, 500, False, 'Cursor survives client disconnect')
        check(compositor.poll() is None, 'Compositor remains healthy')
except Exception:
    if xd is not None:
        image().save(out / 'failure.png')
    raise
finally:
    (out / 'report.json').write_text(json.dumps(checks, indent=2) + '\n')
    if xd is not None:
        xd.close()
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=4)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
    for log in logs:
        log.close()
