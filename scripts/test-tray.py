#!/usr/bin/env python3
"""Tray lifecycle regression on a private D-Bus and Xvfb display.

Build with scripts/build-local.sh, then run with a Python providing dbus-next.
Requires Xvfb; DOME_XVFB and DOME_COMPOSITOR can override executable paths.
"""
import asyncio
import logging
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile

from dbus_next import Message
from dbus_next.aio import MessageBus
from dbus_next.constants import MessageType, PropertyAccess
from dbus_next.errors import DBusError
from dbus_next.service import ServiceInterface, dbus_property

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_TRAY_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session', '--', sys.executable, *sys.argv],
              {**os.environ, 'DOME_TRAY_TEST_BUS': '1'})

ROOT = Path(__file__).resolve().parents[1]
WATCHER = 'org.kde.StatusNotifierWatcher'
PATH = '/org/example/DomeTrayLifecycle'


class Item(ServiceInterface):
    def __init__(self):
        super().__init__('org.kde.StatusNotifierItem')
        self.ready = True
        self.failures = 0
        self.title_reads = 0

    @dbus_property(access=PropertyAccess.READ)
    def Status(self) -> 's':
        if not self.ready:
            self.failures += 1
            raise DBusError('org.freedesktop.DBus.Error.Failed', 'Temporary startup failure')
        return 'Active'

    @dbus_property(access=PropertyAccess.READ)
    def Title(self) -> 's':
        self.title_reads += 1
        return 'Tray lifecycle fixture'


async def wait(predicate):
    deadline = asyncio.get_running_loop().time() + 10
    while asyncio.get_running_loop().time() < deadline:
        if await predicate():
            return
        await asyncio.sleep(.05)
    raise AssertionError('Timed out waiting for tray lifecycle transition')


async def exercise():
    observer = await MessageBus().connect()
    client = await MessageBus().connect()
    item = Item()
    key = client.unique_name + PATH

    async def keys():
        reply = await observer.call(Message(destination=WATCHER,
            path='/StatusNotifierWatcher', interface='org.freedesktop.DBus.Properties',
            member='Get', signature='ss', body=[WATCHER, 'RegisteredStatusNotifierItems']))
        return reply.body[0].value if reply.message_type == MessageType.METHOD_RETURN else None

    async def watcher_ready():
        return await keys() is not None

    async def discovered():
        return item.title_reads > 0

    async def failed():
        return item.failures > 0

    async def removed():
        return key not in (await keys() or [])

    try:
        await wait(watcher_ready)
        reply = await client.call(Message(destination=WATCHER, path='/StatusNotifierWatcher',
            interface=WATCHER, member='RegisterStatusNotifierItem', signature='s', body=[PATH]))
        assert reply.message_type == MessageType.METHOD_RETURN
        await asyncio.sleep(.6)  # Several polls before the object is exported.
        assert key in await keys(), 'Registration was lost before item properties became available'

        client.export(PATH, item)
        await asyncio.sleep(.1)  # Ignore dbus-next's own export-time property enumeration.
        item.title_reads = 0
        await wait(discovered)
        print('PASS: item exported after registration is discovered', flush=True)

        item.ready = False
        await wait(failed)
        await asyncio.sleep(.4)
        assert key in await keys(), 'Temporary property failure removed a live registration'
        item.title_reads = 0
        item.ready = True
        await wait(discovered)
        print('PASS: temporary Status failure recovers without re-registration', flush=True)

        client.disconnect()
        await wait(removed)
        print('PASS: disconnected item is removed from the watcher', flush=True)
    finally:
        client.disconnect()
        observer.disconnect()


logging.basicConfig(level=logging.CRITICAL)  # The fixture deliberately returns D-Bus errors.
children = []
try:
    with tempfile.TemporaryDirectory(prefix='dome-tray-') as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        xvfb = os.environ.get('DOME_XVFB') or shutil.which('Xvfb') or str(ROOT / '.local-deps/root/usr/bin/Xvfb')
        rd, wr = os.pipe()
        out = ROOT / 'artifacts/tray'
        out.mkdir(parents=True, exist_ok=True)
        with (out / 'xvfb.log').open('w') as log:
            children.append(subprocess.Popen([xvfb, '-displayfd', str(wr), '-screen', '0',
                '1280x800x24', '-nolisten', 'tcp', '-noreset'], pass_fds=(wr,), stdout=log, stderr=log))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display = ':' + pipe.readline().strip()
        assert display != ':', 'Xvfb did not start'
        env = {**os.environ, 'XDG_RUNTIME_DIR': directory, 'DISPLAY': display,
            'DOME_LOGIN_SESSION': '0', 'DOME_START_XR': '0', 'DOME_SESSION_RESTORE': '0',
            'XDG_CONFIG_HOME': str(runtime / 'config'), 'XDG_DATA_HOME': str(runtime / 'data'),
            'DOME_PREVIEW_PORT': str(port), 'DOME_RENDERER': 'gles', 'LIBGL_ALWAYS_SOFTWARE': '1'}
        for name in ('WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'QT_QPA_PLATFORM'):
            env.pop(name, None)
        with (out / 'compositor.log').open('w') as log:
            children.append(subprocess.Popen([os.environ.get('DOME_COMPOSITOR',
                str(ROOT / 'target/debug/dome-compositor')), '--backend', 'nested'],
                env=env, cwd=ROOT, stdout=log, stderr=log))
        asyncio.run(exercise())
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
