#!/usr/bin/env python3
"""Exercise the real Miro DBusMenu export through Dome desktop and XR menus.

Run with uv --with python-xlib --with pillow --with dbus-next.
Uses private displays, bus, HOME and runtime; no live session is changed.
"""
import asyncio
import json
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

from PIL import Image, ImageStat
from dbus_next.aio import MessageBus
from dbus_next import Message, MessageType, Variant
from Xlib import X, XK, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_MIRO_MENU_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_MIRO_MENU_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/miro-menus'
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
    raise AssertionError('Timed out; see artifacts/miro-menus')


def check(ok, label):
    checks.append({'check': label, 'passed': bool(ok)})
    print(('PASS ' if ok else 'FAIL ') + label, flush=True)
    assert ok, label


class RuntimeDirectory(tempfile.TemporaryDirectory):
    def __exit__(self, *args):
        stop.set()
        for child in reversed(processes):
            if child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
        subprocess.run(['fusermount3', '-u', str(Path(self.name) / 'doc')],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return super().__exit__(*args)


try:
    with RuntimeDirectory(prefix='dome-miro-menu-') as directory:
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
        def post(path, value):
            request = urllib.request.Request(base + path, data=json.dumps(value).encode(),
                                             headers={'Content-Type': 'application/json'})
            urllib.request.urlopen(request, timeout=3).close()

        async def dbus_call(destination, path, interface, member, signature='', body=None):
            bus = await MessageBus().connect()
            try:
                reply = await asyncio.wait_for(bus.call(Message(destination=destination, path=path,
                                               interface=interface, member=member,
                                               signature=signature, body=body or [])), timeout=3)
                assert reply.message_type != MessageType.ERROR, reply.body
                return reply.body
            finally:
                bus.disconnect()

        def call(service, method, signature='', body=None):
            return asyncio.run(dbus_call(service, '/dev/uden/miro/Menu',
                                        'com.canonical.dbusmenu', method, signature, body))

        def service_for(pid):
            names = asyncio.run(dbus_call('org.freedesktop.DBus', '/org/freedesktop/DBus',
                                         'org.freedesktop.DBus', 'ListNames'))[0]
            for name in reversed(names):
                if not name.startswith(':'):
                    continue
                try:
                    owner_pid = asyncio.run(dbus_call('org.freedesktop.DBus', '/org/freedesktop/DBus',
                                                     'org.freedesktop.DBus', 'GetConnectionUnixProcessID', 's', [name]))[0]
                except AssertionError:
                    continue
                if owner_pid == pid:
                    try:
                        call(name, 'GetLayout', 'iias', [0, -1, []])
                        return name
                    except (AssertionError, TimeoutError):
                        pass

        def model(service):
            return call(service, 'GetLayout', 'iias', [0, -1, []])[1]

        def children(node):
            return [variant.value for variant in node[2]]

        def item(service, label):
            def find(node):
                if node[1].get('label', Variant('s', '')).value == label:
                    return node
                return next((result for child in children(node) if (result := find(child))), None)
            return find(model(service))

        def checked(service, label):
            return item(service, label)[1]['toggle-state'].value == 1

        def launch(name, pdf=True):
            existing = {w['id'] for w in get('/api/state')['scene']['windows']}
            process = start([binary, *([str(sample)] if pdf else [])], name, miro_env, stdin=subprocess.DEVNULL)
            window = wait(lambda: next((w for w in get('/api/state')['scene']['windows']
                                        if w['app_id'] == 'miro-pdf' and w['id'] not in existing), None))
            service = wait(lambda: service_for(process.pid))
            # Wait until the Wayland attachment task has run and the menu model is current.
            time.sleep(1)
            return window, service, process

        xd = display.Display(ds)
        host = wait(lambda: next((w for w in xd.screen().root.query_tree().children
                                 if str(w.get_wm_name() or '').startswith('Dome')), None))
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()

        def motion(x, y):
            g = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=int(g.x+x), y=int(g.y+y))
            xd.sync()
            time.sleep(.15)

        def click(x, y):
            motion(x, y)
            for kind in [X.ButtonPress, X.ButtonRelease]:
                xtest.fake_input(xd, kind, 1)
                xd.sync()
                time.sleep(.1)

        def key(name):
            code = xd.keysym_to_keycode(XK.string_to_keysym(name))
            for kind in [X.KeyPress, X.KeyRelease]:
                xtest.fake_input(xd, kind, code)
                xd.sync()
                time.sleep(.1)

        # Dome's common header geometry: File, Edit, View, Layout, Window.
        headers = {'File': 255, 'Edit': 306, 'View': 360, 'Layout': 426, 'Window': 502}

        def select(service, category, label, pointer=click):
            category_node = next(n for n in children(model(service)) if n[1]['label'].value == category)
            index = next(i for i, n in enumerate(children(category_node)) if n[1].get('label', Variant('s', '')).value == label)
            pointer(headers[category], 18)
            pointer(headers[category] + 15, 40 + 28 * index + 14)

        def pixels(window, name):
            with urllib.request.urlopen(base + f'/api/xr-frame/{window["id"]}', timeout=3) as response:
                im = Image.frombytes('RGBA', (int(response.headers['X-Dome-Width']),
                                            int(response.headers['X-Dome-Height'])), response.read())
            im.save(out / (name + '.png'))
            return im

        first, a, first_process = launch('first')
        check([n[1]['label'].value for n in children(model(a))] == ['File', 'View', 'Layout'],
              'Miro publishes its shared File, View and Layout menus over DBusMenu')
        check(item(a, 'Open File')[1]['shortcut'].value == [['Control', 'o']],
              'Exporter publishes the configured Open shortcut')
        recent = children(item(a, 'Recent Files'))
        check(len(recent) == 1 and recent[0][1]['label'].value == sample.name.replace('_', '__'),
              'Recent documents are exported as a nested menu with escaped labels')
        select(a, 'View', 'Toggle PDF Dark Mode')
        wait(lambda: not checked(a, 'Toggle PDF Dark Mode'))
        def page_mean(window, name):
            image = pixels(window, name)
            return sum(ImageStat.Stat(image.crop((image.width*.4, image.height*.4, image.width*.6, image.height*.6))).mean[:3])/3
        wait(lambda: page_mean(first, 'desktop-light-pdf') > 180)
        check(True,
              'Desktop universal menu invokes the real PDF action and changes rendered pixels')
        select(a, 'Layout', 'Double Page')
        wait(lambda: checked(a, 'Double Page'))
        check(not checked(a, 'Single Page'), 'Layout radio state updates after a global-menu action')
        # Exercise the compositor keyboard path after opening the native View model.
        click(headers['View'], 18)
        key('Home'); key('Return')
        wait(lambda: not checked(a, 'Toggle Interface Dark Mode'))
        check(True, 'Keyboard navigation activates a native Miro command')

        second, b, second_process = launch('second')
        select(b, 'View', 'Toggle PDF Dark Mode')
        wait(lambda: not checked(b, 'Toggle PDF Dark Mode'))
        check(not checked(a, 'Toggle PDF Dark Mode'), 'Focus switches to the second process exporter')
        select(b, 'View', 'Toggle PDF Dark Mode')
        wait(lambda: checked(b, 'Toggle PDF Dark Mode'))
        check(not checked(a, 'Toggle PDF Dark Mode'), 'Only the focused Miro process receives menu activation')

        (config / 'miro-pdf/recent_files.json').write_text(json.dumps({'files': [str(sample)]}))
        empty, c, empty_process = launch('empty', pdf=False)
        check(not item(c, 'Print')[1]['enabled'].value and not item(c, 'Close')[1]['enabled'].value,
              'Document actions are disabled when no PDF is open')
        # Recent submenu pointer traversal reopens the document in the empty window.
        click(headers['File'], 18)
        category = next(n for n in children(model(c)) if n[1]['label'].value == 'File')
        recent_index = next(i for i,n in enumerate(children(category)) if n[0] == item(c, 'Recent Files')[0])
        motion(270, 54 + 28 * recent_index)
        click(635, 54 + 28 * recent_index)
        wait(lambda: item(c, 'Print')[1]['enabled'].value)
        check(True, 'Nested Recent Files menu opens a PDF through the existing application action')

        # Without a Wayland menu host, the normal in-window menu remains usable.
        fallback_env = {**env, 'DISPLAY': ds}
        fallback_env.pop('WAYLAND_DISPLAY', None)
        fallback_env.pop('WAYLAND_SOCKET', None)
        fallback = start([binary, str(sample)], 'x11-fallback', fallback_env, stdin=subprocess.DEVNULL)
        native = wait(lambda: next((w for w in xd.screen().root.query_tree().children
                                    if w.get_wm_name() == 'Miro'), None))
        fallback_service = wait(lambda: service_for(fallback.pid))
        native.configure(stack_mode=X.Above)
        native.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        time.sleep(1)
        def native_click(x, y):
            g = native.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=int(g.x+x), y=int(g.y+y))
            for kind in [X.ButtonPress, X.ButtonRelease]:
                xtest.fake_input(xd, kind, 1)
                xd.sync()
                time.sleep(.15)
        g = native.get_geometry()
        raw = native.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xffffffff)
        Image.frombytes('RGB', (g.width, g.height), raw.data, 'raw', 'BGRX').save(out / 'x11-fallback.png')
        native_click(70, 14)
        native_click(100, 70)
        wait(lambda: not checked(fallback_service, 'Toggle PDF Dark Mode'))
        check(True, 'Miro retains working in-window menus without a Wayland menu host')
        fallback.terminate()
        fallback.wait(timeout=5)
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()

        def heartbeat():
            while not stop.wait(.15):
                try:
                    post('/api/xr-session', {'visible': True, 'focused': True})
                except OSError:
                    pass
        post('/api/xr-session', {'visible': True, 'focused': True})
        threading.Thread(target=heartbeat, daemon=True).start()
        wait(lambda: get('/api/xr-control')['input_realm'] == 'xr')
        post('/api/xr-input', {'events': [], 'actions': [{'op': 'focus', 'id': second['id']}]})
        wait(lambda: get('/api/state')['scene']['focused'] == second['id'])
        time.sleep(.5)

        def xr_click(x, y):
            layout = get('/api/xr-shell')
            post('/api/xr-input', {'events': [{'kind': 'motion', 'id': 0,
                                              'uv': [x/layout['width'], y/layout['top']]}], 'actions': []})
            time.sleep(.15)
            post('/api/xr-input', {'events': [{'kind': 'button', 'button': 272, 'pressed': True},
                                              {'kind': 'button', 'button': 272, 'pressed': False}], 'actions': []})
            time.sleep(.25)

        select(b, 'View', 'Toggle PDF Dark Mode', xr_click)
        wait(lambda: not checked(b, 'Toggle PDF Dark Mode'))
        check(not checked(a, 'Toggle PDF Dark Mode'), 'XR universal menu dispatches only to the focused Miro exporter')
        wait(lambda: page_mean(second, 'xr-light-pdf') > 180)
        # The compositor Window menu remains available alongside native categories.
        xr_click(headers['Window'], 18)
        xr_click(headers['Window'] + 10, 54)
        wait(lambda: next(w for w in get('/api/state')['scene']['windows'] if w['id'] == second['id'])['xr_visible'] is False)
        check(True, 'Compositor Window presentation controls remain available beside native Miro menus')
        second_process.terminate()
        second_process.wait(timeout=5)
        wait(lambda: all(w['id'] != second['id'] for w in get('/api/state')['scene']['windows']))
        post('/api/xr-input', {'events': [], 'actions': [{'op': 'focus', 'id': first['id']}]})
        time.sleep(.5)
        select(a, 'View', 'Toggle PDF Dark Mode', xr_click)
        wait(lambda: checked(a, 'Toggle PDF Dark Mode'))
        check(True, 'Exporter disconnect removes its window without leaving stale Miro menu routing')
        check(preferences.read_text() == original, 'Menu operations preserve saved Miro preferences')
        (out / 'report.json').write_text(json.dumps({'checks': checks}, indent=2))
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
