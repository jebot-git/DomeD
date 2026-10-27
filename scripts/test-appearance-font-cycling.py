#!/usr/bin/env python3
"""Stress native font changes against toolkit exports on a private desktop.

Requires Xvfb, python-xlib and Pillow. DOME_COMPOSITOR selects the release binary.
No running session or user configuration is changed.
"""
import json
import os
from pathlib import Path
import resource
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

from Xlib import X, XK, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])
resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
if os.environ.get('DOME_FONT_CYCLING_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_FONT_CYCLING_BUS': '1'})

root = Path(__file__).resolve().parents[1]
http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
children = []


def wait(check, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if children and children[-1].poll() is not None:
            raise AssertionError('Private compositor exited during font cycling')
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError):
            pass
        time.sleep(.03)
    raise AssertionError('Timed out waiting for private appearance update')


try:
    with tempfile.TemporaryDirectory(prefix='dome-font-cycling-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        config = runtime / 'config/dome'
        config.mkdir(parents=True)
        appearance = config / 'appearance.json'
        appearance.write_text(json.dumps({'font': 'Galmuri9 / Regular', 'font_size': 12}))
        env = {**os.environ, 'HOME': str(runtime), 'XDG_RUNTIME_DIR': directory,
               'XDG_CONFIG_HOME': str(config.parent), 'XDG_CACHE_HOME': str(runtime / 'cache'),
               'XDG_DATA_HOME': str(runtime / 'data'), 'XDG_STATE_HOME': str(runtime / 'state'),
               'DOME_LOGIN_SESSION': '0', 'DOME_SESSION_RESTORE': '0', 'DOME_APPEARANCE_SERVICE': '1',
               'LIBGL_ALWAYS_SOFTWARE': '1', 'DOME_RENDERER': 'gles', 'RUST_LOG': 'warn'}
        for name in ['XR', 'TRAY', 'KDECONNECT', 'POLKIT', 'KWALLET', 'IME']:
            env['DOME_START_' + name] = '0'
        for name in ['DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'FONTCONFIG_FILE']:
            env.pop(name, None)
        rd, wr = os.pipe()
        children.append(subprocess.Popen(['Xvfb', '-displayfd', str(wr), '-screen', '0',
                                          '1280x800x24', '-nolisten', 'tcp', '-noreset'],
                                         pass_fds=(wr,), env=env, stdout=subprocess.DEVNULL))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            env['DISPLAY'] = ':' + pipe.readline().strip()
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            env['DOME_PREVIEW_PORT'] = str(sock.getsockname()[1])
        base = 'http://127.0.0.1:' + env['DOME_PREVIEW_PORT']
        with tempfile.TemporaryFile(mode='w+') as log:
            compositor = subprocess.Popen([os.environ.get('DOME_COMPOSITOR',
                                           str(root / 'target/release/dome-compositor')),
                                           '--backend', 'nested'], env=env, stdout=log,
                                          stderr=subprocess.STDOUT)
            children.append(compositor)
            try:
                wait(lambda: http.open(base + '/api/state', timeout=2).status == 200)
                xd = display.Display(env['DISPLAY'])
                host = wait(lambda: next((w for w in xd.screen().root.query_tree().children
                                          if str(w.get_wm_name() or '').startswith('Dome')), None))
                host.set_input_focus(X.RevertToParent, X.CurrentTime)
                xd.sync()

                def click(x, y):
                    g = host.get_geometry()
                    xtest.fake_input(xd, X.MotionNotify, x=g.x + x, y=g.y + y)
                    for kind in [X.ButtonPress, X.ButtonRelease]:
                        xtest.fake_input(xd, kind, 1)
                        xd.sync()
                        time.sleep(.08)

                def key(name):
                    code = xd.keysym_to_keycode(XK.string_to_keysym(name))
                    for kind in [X.KeyPress, X.KeyRelease]:
                        xtest.fake_input(xd, kind, code)
                    xd.sync()

                def settings(): return json.loads(appearance.read_text())
                def panel_height(): return min(752, host.get_geometry().height-112)
                def row(n):
                    g=host.get_geometry()
                    click(g.width//2+100,44+(g.height-112-panel_height())//2+60+n*48+24)
                # Settings now owns the Appearance entry. Use its standard shortcut
                # and first keyboard-selected row rather than an obsolete menu offset.
                super_code = xd.keysym_to_keycode(XK.string_to_keysym('Super_L'))
                xtest.fake_input(xd, X.KeyPress, super_code)
                key('comma')
                xtest.fake_input(xd, X.KeyRelease, super_code); xd.sync()
                time.sleep(.3)
                key('Return'); time.sleep(.3)
                g = host.get_geometry()
                for expected in ['Galmuri11 / Regular','Galmuri14 / Regular','Galmuri7 / Regular','Galmuri9 / Regular']:
                    row(5);wait(lambda:settings()['font'].split(' [')[0]==expected)
                key('Left');wait(lambda:settings()['font']=='Galmuri7 / Regular')
                families = json.loads((root / 'assets/fonts/collections.json').read_text())['families']
                visited = set()
                for _ in range(len(families) + 1):
                    visited.add(settings()['font'].split(' / ')[0])
                    if settings()['font'].startswith('Fusion Pixel '):break
                    before_font = settings()['font']
                    row(3)
                    wait(lambda: settings()['font'] != before_font)
                assert settings()['font'].startswith('Fusion Pixel '), (settings(), visited)
                for family in ['Tekuplus', 'x12y12pxMaruMinyaHangul', 'x5y8pxNegaTape',
                               'x5y8pxNegaClip', 'DomeD Shijimi', 'x14y24pxHeadUpDaisy',
                               'Ark Pixel 12px Mono', 'Jelly Pixel 12px Mono', 'Retro Pixel Cute Mono', 'x12y12pxMaruMinya']:
                    assert any(v == family or (family.split(' Pixel')[0]+' Pixel' in v if ' Pixel' in family else False) for v in visited), f'New font family absent from picker: {family}'
                old=settings();native=old['font_size']
                row(5);wait(lambda:settings()['font_size']!=native)
                assert settings()['font'].split(' / ')[0].endswith(('Mono','Prop')), settings()
                from PIL import Image
                g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
                artifact=root/'artifacts/bitmap-fonts';artifact.mkdir(parents=True,exist_ok=True)
                Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(artifact/'mixed-script-selection.png')
                print('PASS: Galmuri strike cycling/wrap, combined Fusion Pixel strikes and adaptive Appearance panel',flush=True)
                changes = set()
                for index in range(160):
                    before = appearance.read_text()
                    # Family, face, size, and scale all enter native font caches.
                    if index % 8 == 0:
                        click(g.width // 2 + 100,
                              (g.height - panel_height()) // 2 + 60 + (3 + index // 8 % 4) * 48 + 24)
                    else:
                        key('Right')
                    time.sleep(.04)
                    assert compositor.poll() is None, 'Compositor crashed during font switching'
                    after = appearance.read_text()
                    changes.add(after)
                    if index % 8 == 0:
                        wait(lambda: appearance.read_text() != before or index // 8 % 4 in (1, 2))
                assert len(changes) >= 15, f'Expected varied font changes, got {len(changes)}'
                time.sleep(2)
                portal = subprocess.check_output([
                    'gdbus', 'call', '--session', '--dest', 'org.freedesktop.impl.portal.desktop.dome',
                    '--object-path', '/org/freedesktop/portal/desktop', '--method',
                    'org.freedesktop.impl.portal.Settings.Read', 'org.gnome.desktop.interface', 'font-name'],
                    env=env, text=True, timeout=5)
                assert portal.strip(), 'Appearance worker stopped responding'
                assert (runtime / 'dome-fonts/fonts.conf').is_file()
                assert compositor.poll() is None
                print(f'PASS: 160 native font changes; {len(changes)} distinct settings; '
                      'compositor and appearance portal remain responsive')
                xd.close()
            except BaseException:
                log.seek(0)
                print(log.read()[-12000:], file=sys.stderr)
                raise
                # Stop writers before TemporaryDirectory removes exported fonts.
            finally:
                compositor.terminate() if compositor.poll() is None else None
                try:
                    compositor.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    compositor.kill(); compositor.wait()
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
