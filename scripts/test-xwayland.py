#!/usr/bin/env python3
"""Exercise the login session's Xwayland bridge on a private Xvfb and D-Bus."""
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
if os.environ.get('DOME_XWAYLAND_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_XWAYLAND_TEST_BUS': '1'})
out = ROOT / 'artifacts/xwayland'
out.mkdir(parents=True, exist_ok=True)

def wait(predicate):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/xwayland')

children = []
try:
    with tempfile.TemporaryDirectory(prefix='dome-xwayland-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        xvfb = os.environ.get('DOME_XVFB') or shutil.which('Xvfb') or str(ROOT / '.local-deps/root/usr/bin/Xvfb')
        rd, wr = os.pipe()
        with (out / 'xvfb.log').open('w') as log:
            xserver = subprocess.Popen([xvfb, '-displayfd', str(wr), '-screen', '0', '1280x800x24', '-nolisten', 'tcp'], pass_fds=(wr,), stdout=log, stderr=log)
        children.append(xserver)
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display = ':' + pipe.readline().strip()
        env = {**os.environ, 'XDG_RUNTIME_DIR': directory, 'DISPLAY': display,
               'DOME_LOGIN_SESSION': '1', 'DOME_START_XR': '0', 'DOME_SESSION_RESTORE': '0',
               'DOME_START_XWAYLAND': '1', 'DOME_DRM_TEST_DEVICE': '/dev/null',
               'DOME_PREVIEW_PORT': str(port), 'DOME_RENDERER': 'gles', 'LIBGL_ALWAYS_SOFTWARE': '1',
               'XDG_CONFIG_HOME': str(runtime / 'config'), 'XDG_DATA_HOME': str(runtime / 'data'),
               'XDG_DATA_DIRS': str(runtime / 'empty'), 'RUST_LOG': 'warn'}
        for key in ('WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'GDK_BACKEND', 'SDL_VIDEODRIVER'):
            env.pop(key, None)
        with (out / 'compositor.log').open('w') as log:
            compositor = subprocess.Popen([os.environ.get('DOME_COMPOSITOR', str(ROOT / 'target/debug/dome-compositor')), '--backend', 'nested'], env=env, stdout=log, stderr=log, start_new_session=True)
        children.append(compositor)
        match = wait(lambda: re.search(r'Dome Xwayland bridge: (:\d+)', (out / 'compositor.log').read_text()))
        bridge_display = match[1]
        wait(lambda: Path('/tmp/.X11-unix/X' + bridge_display[1:]).exists())
        with (out / 'client.log').open('w') as log:
            client = subprocess.Popen(['xmessage', '-title', 'Dome X11 bridge fixture', 'X11 inside Dome'], env={**env, 'DISPLAY': bridge_display}, stdout=log, stderr=log)
        children.append(client)
        def mapped():
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state', timeout=2) as response:
                data=json.load(response)
                (out / 'state.json').write_text(json.dumps(data))
                return any(w['title'] == 'Dome X11 bridge fixture' and w['origin'] == 'desktop' and w['desktop_visible'] and not w['xr_visible'] for w in data['scene']['windows'])
        wait(mapped)
        # The native game mirror follows the same rule even when launched from XR.
        xr_socket = wait(lambda: re.search(r'Dome XR Wayland socket: (\S+)', (out / 'compositor.log').read_text()))[1]
        protocol = '/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml'
        subprocess.run(['wayland-scanner', 'client-header', protocol, str(runtime / 'xdg-shell-client-protocol.h')], check=True)
        subprocess.run(['wayland-scanner', 'private-code', protocol, str(runtime / 'xdg-shell-protocol.c')], check=True)
        fixture = (ROOT / 'scripts/refresh-client.c').read_text().replace('"dome-refresh-probe"', '"vrchat.exe"')
        (runtime / 'mirror.c').write_text(fixture)
        subprocess.run(['cc', '-O2', '-I' + directory, str(runtime / 'mirror.c'),
            str(runtime / 'xdg-shell-protocol.c'), '-lwayland-client', '-o', str(runtime / 'mirror')], check=True)
        with (out / 'mirror-client.log').open('w') as log:
            mirror = subprocess.Popen([str(runtime / 'mirror')],
                env={**env, 'WAYLAND_DISPLAY': xr_socket}, stdout=log, stderr=log)
        children.append(mirror)
        def mirror_mapped():
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state', timeout=2) as response:
                data = json.load(response)
                (out / 'mirror-state.json').write_text(json.dumps(data))
                return any(w['app_id'] == 'vrchat.exe' and w['origin'] == 'desktop'
                    and w['desktop_visible'] and not w['xr_visible'] for w in data['scene']['windows'])
        wait(mirror_mapped)
        compositor.send_signal(signal.SIGTERM)
        compositor.wait(timeout=10)
        wait(lambda: not Path('/tmp/.X11-unix/X' + bridge_display[1:]).exists())
        print('PASS: X11 and native VRChat mirror clients stay desktop-only; Xwayland stops on logout')
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
    if 'compositor' in globals():
        try:
            os.killpg(compositor.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
