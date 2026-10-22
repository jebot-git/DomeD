#!/usr/bin/env python3
"""Native FLTK tray/editor tests on private X11 and DomeD Wayland displays."""
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

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('TRANSLATOR_PRIVATE_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
               {**os.environ, 'TRANSLATOR_PRIVATE_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[2]
build = Path(os.environ.get('DOME_TRANSLATOR_BUILD_DIR', root / 'build/translator'))
artifacts = root / 'artifacts/translator'
artifacts.mkdir(parents=True, exist_ok=True)
children = []
logs = []


def start(args, env, name, **kwargs):
    log = (artifacts / (name + '.log')).open('w')
    logs.append(log)
    p = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    children.append(p)
    return p


def wait(fn):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/translator')


try:
    with tempfile.TemporaryDirectory(prefix='dome-translator-test-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        config = runtime / 'config/dome'
        config.mkdir(parents=True)
        (config / 'appearance.json').write_text(json.dumps({'font': 'Unifont / Sans-Serif', 'font_size': 16, 'font_scale': 1, 'light': False}))
        env = {**os.environ, 'XDG_RUNTIME_DIR': directory, 'XDG_CONFIG_HOME': str(runtime / 'config'),
               'XDG_CACHE_HOME': str(runtime / 'cache'), 'XDG_STATE_HOME': str(runtime / 'state'),
               'XDG_CURRENT_DESKTOP': 'Dome', 'DOME_LOGIN_SESSION': '0', 'DOME_SESSION_RESTORE': '0',
               'DOME_START_XR': '0', 'DOME_START_XWAYLAND': '0', 'LIBGL_ALWAYS_SOFTWARE': '1',
               'DOME_START_TRAY': '0', 'DOME_START_POLKIT': '0', 'DOME_START_KDECONNECT': '0',
               'GIO_USE_VFS': 'local', 'RUST_LOG': 'warn'}
        for name in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DISPLAY', 'DEEPL_API_KEY']:
            env.pop(name, None)
        rd, wr = os.pipe()
        start(['Xvfb', '-displayfd', str(wr), '-screen', '0', '1600x1000x24', '-nolisten', 'tcp', '-noreset'], env, 'xvfb', pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            env['DISPLAY'] = ':' + pipe.readline().strip()
        x11 = start([str(build / 'translator-test-driver')], {**env, 'FLTK_BACKEND': 'x11'}, 'x11')
        assert x11.wait(timeout=25) == 0, (artifacts / 'x11.log').read_text()
        print('PASS isolated X11: editor, clipboard, primary, async results, single instance, tray menu', flush=True)
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            env['DOME_PREVIEW_PORT'] = str(s.getsockname()[1])
        compositor = start([str(root / 'target/release/dome-compositor'), '--backend', 'nested'], env, 'compositor')
        def wayland():
            match = re.search(r'Dome Wayland socket: (\S+)', (artifacts / 'compositor.log').read_text())
            return match[1] if match else None
        env['WAYLAND_DISPLAY'] = wait(wayland)
        wl = start([str(build / 'translator-test-driver')], env, 'wayland')
        assert wl.wait(timeout=30) == 0, (artifacts / 'wayland.log').read_text()
        print('PASS isolated DomeD Wayland: independent UTF-8 clipboard and primary selection; native tray lifecycle', flush=True)
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
    for child in reversed(children):
        try:
            child.wait(timeout=4)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    for log in logs:
        log.close()
