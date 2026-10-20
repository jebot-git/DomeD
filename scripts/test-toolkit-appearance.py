#!/usr/bin/env python3
"""Real GTK3/GTK4 and Qt session-theme propagation on a private Wayland display.
Build scripts/toolkit-client.cpp with Qt6Widgets, then run with uv --with python-xlib --with pillow.
The system Python needs GTK introspection. No host theme settings are changed.
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

if '--gtk-fixture' in sys.argv:
    import gi
    version, output = sys.argv[2:4]
    gi.require_version('Gtk', version)
    from gi.repository import Gtk, GLib
    if version == '3.0':
        Gtk.init([])
        window = Gtk.Window(title='GTK3 appearance fixture')
        window.add(Gtk.Label(label='Session appearance · 日本語 · Žluťoučký'))
        window.set_default_size(480, 200)
        window.show_all()
    else:
        Gtk.init()
        window = Gtk.Window(title='GTK4 appearance fixture')
        window.set_child(Gtk.Label(label='Session appearance · 日本語 · Žluťoučký'))
        window.set_default_size(480, 200)
        window.present()
    def record():
        settings = Gtk.Settings.get_default()
        context = window.get_style_context()
        _, bg = context.lookup_color('theme_bg_color')
        _, accent = context.lookup_color('theme_selected_bg_color')
        value = {'font': settings.get_property('gtk-font-name'), 'theme': settings.get_property('gtk-theme-name'),
                 'window': [bg.red, bg.green, bg.blue], 'accent': [accent.red, accent.green, accent.blue]}
        temporary = Path(output + '.tmp')
        temporary.write_text(json.dumps(value))
        temporary.replace(output)
        return True
    GLib.timeout_add(100, record)
    GLib.MainLoop().run()
    raise SystemExit

from Xlib import X, display
from Xlib.ext import xtest
from PIL import Image
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_TOOLKIT_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_TOOLKIT_TEST_BUS': '1'})
root = Path(__file__).resolve().parents[1]
out = root / 'artifacts/toolkit-appearance'
out.mkdir(parents=True, exist_ok=True)
processes, logs = [], []


def start(args, name, env, **kwargs):
    log = (out / (name + '.log')).open('w')
    logs.append(log)
    child = subprocess.Popen(args, env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child


def wait(check, timeout=25):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/toolkit-appearance')


def read(name):
    return json.loads((out / (name + '.json')).read_text())

class RuntimeDirectory(tempfile.TemporaryDirectory):
    def __exit__(self, *args):
        # The private documents portal may mount a FUSE export below its runtime directory.
        subprocess.run(['fusermount3', '-u', str(Path(self.name) / 'doc')], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return super().__exit__(*args)


try:
    with RuntimeDirectory(prefix='dome-toolkit-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        config = runtime / 'config'
        (config / 'dome').mkdir(parents=True)
        (config / 'dome/keyboard.json').write_text(json.dumps({'ime': False, 'layout': 'us'}))
        (config / 'dome/appearance.json').write_text(json.dumps({'light': False, 'font': 'GNU Unifont / Sans-Serif', 'font_size': 16}))
        (config / 'xdg-desktop-portal').mkdir()
        (config / 'xdg-desktop-portal/dome-portals.conf').write_text((root / 'packaging/dome-portals.conf').read_text())
        apps = runtime / 'data/applications'
        apps.mkdir(parents=True)
        fixtures = {
            'qt': str(out / 'qt-client') + ' ' + str(out / 'qt.json'),
            'gtk3': '/usr/bin/python3 ' + str(Path(__file__).resolve()) + ' --gtk-fixture 3.0 ' + str(out / 'gtk3.json'),
            'gtk4': '/usr/bin/python3 ' + str(Path(__file__).resolve()) + ' --gtk-fixture 4.0 ' + str(out / 'gtk4.json'),
            'gtk3-native': '/usr/bin/env GTK_USE_PORTAL=0 /usr/bin/python3 ' + str(Path(__file__).resolve()) + ' --gtk-fixture 3.0 ' + str(out / 'gtk3-native.json'),
        }
        for name, command in fixtures.items():
            (out / (name + '.json')).unlink(missing_ok=True)
            (apps / (name + '.desktop')).write_text('[Desktop Entry]\nType=Application\nName=Dome Toolkit ' + name + '\nExec=/usr/bin/env QT_QPA_PLATFORM=wayland GDK_BACKEND=wayland ' + command + '\n')
        (out / 'qt-new.json').unlink(missing_ok=True)
        (apps / 'qt-new.desktop').write_text('[Desktop Entry]\nType=Application\nName=Dome Toolkit qt-new\nExec=/usr/bin/env QT_QPA_PLATFORM=wayland ' + str(out / 'qt-client') + ' ' + str(out / 'qt-new.json') + '\n')
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
        env = {**os.environ, 'DISPLAY': ds, 'XDG_RUNTIME_DIR': directory, 'XDG_CONFIG_HOME': str(config),
               'XDG_DATA_HOME': str(runtime / 'data'), 'XDG_DATA_DIRS': '/usr/local/share:/usr/share',
               'XDG_CURRENT_DESKTOP': 'Dome', 'XDG_SESSION_DESKTOP': 'dome', 'XDG_SESSION_TYPE': 'wayland',
               'DOME_LOGIN_SESSION': '0', 'DOME_BACKEND': 'nested', 'DOME_APPEARANCE_SERVICE': '1',
               'DOME_START_XR': '0', 'DOME_START_IME': '0', 'DOME_PREVIEW_PORT': str(port), 'DOME_RENDERER': 'gles',
               'XDG_DESKTOP_PORTAL_DIR': str(root / 'packaging'), 'LIBGL_ALWAYS_SOFTWARE': '1',
               'GI_TYPELIB_PATH': os.pathsep.join(filter(None, [os.environ.get('GI_TYPELIB_PATH'),
                   str(root / 'build/toolkit-deps/root/usr/lib/x86_64-linux-gnu/girepository-1.0')])),
               'NO_AT_BRIDGE': '1', 'GIO_USE_VFS': 'local', 'RUST_LOG': 'warn'}
        for key in ['WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM', 'QT_QPA_PLATFORMTHEME', 'GTK_THEME', 'FONTCONFIG_FILE']:
            env.pop(key, None)
        # The frontend is activated on this private bus, after Dome has registered its Settings backend.
        subprocess.run(['dbus-update-activation-environment', 'XDG_CONFIG_HOME', 'XDG_RUNTIME_DIR',
                        'XDG_DATA_HOME', 'XDG_DATA_DIRS', 'XDG_CURRENT_DESKTOP', 'XDG_SESSION_DESKTOP',
                        'XDG_SESSION_TYPE', 'XDG_DESKTOP_PORTAL_DIR', 'NO_AT_BRIDGE', 'DISPLAY'], env=env, check=True)
        compositor = start([os.environ.get('DOME_COMPOSITOR', str(root / 'target/debug/dome-compositor')), '--backend', 'nested'], 'compositor', env)
        def get(path):
            return json.load(urllib.request.urlopen(base + path, timeout=2))
        def post(path, value):
            request = urllib.request.Request(base + path, data=json.dumps(value).encode(), headers={'Content-Type': 'application/json'})
            urllib.request.urlopen(request, timeout=2).close()
        wait(lambda: get('/api/state')['scene']['source'] == 'wayland')
        catalog = get('/api/apps')
        for name in fixtures:
            app = next(app for app in catalog if app['name'] == 'Dome Toolkit ' + name)
            post('/api/launch', {'id': app['id']})
        dark = {name: wait(lambda name=name: read(name)) for name in fixtures}
        def colour_matches(rgb, colour):
            return all(abs(channel - int(colour[index:index+2], 16) / 255) < .001
                       for channel, index in zip(rgb, [1, 3, 5]))
        gtk_names = ['gtk3', 'gtk4', 'gtk3-native']
        assert dark['qt']['family'] == 'GNU Unifont' and dark['qt']['pixels'] == 16, dark
        assert dark['qt']['window'] == '#1f252d', dark
        assert all(dark[name]['font'] == 'GNU Unifont 12' and colour_matches(dark[name]['window'], '#1f252d')
                   and colour_matches(dark[name]['accent'], '#6babff') for name in gtk_names), dark
        xd = display.Display(ds)
        host = wait(lambda: next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')), None))
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        def click(x, y):
            g = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=g.x+x, y=g.y+y)
            xd.sync(); time.sleep(.1)
            for kind in [X.ButtonPress, X.ButtonRelease]:
                xtest.fake_input(xd, kind, 1); xd.sync(); time.sleep(.1)
        click(20, 18); click(100, 222)  # Dome > Appearance
        click(750, 230)  # Light mode, delivered through the normal appearance UI.
        wait(lambda: read('qt')['window'] == '#e5e9ef')
        wait(lambda: read('gtk3')['theme'] != dark['gtk3']['theme'])
        click(750, 278)  # Accent
        wait(lambda: read('qt')['accent'] == '#61cfa5')
        click(750, 425)  # Next bitmap font
        wait(lambda: read('qt')['family'] == 'Cozette' and read('qt')['pixels'] == 13)
        wait(lambda: all(read(name)['font'] != dark[name]['font']
                         and colour_matches(read(name)['window'], '#e5e9ef')
                         and colour_matches(read(name)['accent'], '#61cfa5') for name in gtk_names))
        changed = {name: read(name) for name in fixtures}
        assert all(changed[name]['font'] == 'Cozette 9.75' for name in gtk_names), changed
        # New launches must read the current settings too, through the same launch path.
        app = next(app for app in catalog if app['name'] == 'Dome Toolkit qt-new')
        post('/api/launch', {'id': app['id']})
        wait(lambda: read('qt-new')['window'] == '#e5e9ef' and read('qt-new')['family'] == changed['qt']['family'])
        g = host.get_geometry()
        raw = host.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xffffffff)
        Image.frombytes('RGB', (g.width, g.height), raw.data, 'raw', 'BGRX').save(out / 'themed-apps.png')
        (out / 'report.json').write_text(json.dumps({'dark': dark, 'changed': changed}, indent=2))
        print('PASS: spawned Qt/GTK3/GTK4 windows inherit session fonts and colours; live mode/accent/font changes and subsequent launches match')
finally:
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
    for child in reversed(processes):
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
    for log in logs:
        log.close()
