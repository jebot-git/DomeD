#!/usr/bin/env python3
"""Exercise the real portal frontend and Request.Close on a private session bus."""
import asyncio
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from dbus_next import Message, MessageType, Variant
from dbus_next.aio import MessageBus

async def main():
    assert os.environ.get('DOME_PRIVATE_SERVICE_TEST') == '1'
    executable=Path(sys.argv[1]).resolve()
    output=Path(sys.argv[2]).resolve();output.mkdir(parents=True,exist_ok=True)
    with tempfile.TemporaryDirectory() as directory:
        config=Path(directory)
        (config/'xdg-desktop-portal').mkdir()
        (config/'xdg-desktop-portal/portals.conf').write_text('[preferred]\ndefault=fltk\n')
        portals=config/'portals';portals.mkdir()
        (portals/'fltk.portal').write_text('[portal]\nDBusName=org.freedesktop.impl.portal.desktop.fltk\nInterfaces=org.freedesktop.impl.portal.FileChooser;org.freedesktop.impl.portal.Access;org.freedesktop.impl.portal.AppChooser;\nUseIn=Dome;\n')
        (portals/'portals.conf').write_text('[preferred]\ndefault=fltk\n')
        (config/'dome').mkdir()
        shutil.copy2(Path.home()/'.config/dome/appearance.json',config/'dome/appearance.json')
        env={**os.environ,'XDG_CONFIG_HOME':str(config),'XDG_DESKTOP_PORTAL_DIR':str(portals),'XDG_CURRENT_DESKTOP':'Dome'}
        log=(output/'frontend-private.log').open('w')
        backend=subprocess.Popen([executable],env=env,stdout=log,stderr=subprocess.STDOUT)
        bus=await MessageBus().connect()
        for _ in range(250):
            result=await bus.call(Message(destination='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='NameHasOwner',signature='s',body=['org.freedesktop.impl.portal.desktop.fltk']))
            if result.body==[True]:break
            await asyncio.sleep(.04)
        else:
            backend.terminate();backend.wait(timeout=5)
            raise AssertionError('FLTK backend did not start')
        portal=subprocess.Popen(['/usr/libexec/xdg-desktop-portal','--verbose'],env=env,stdout=log,stderr=subprocess.STDOUT)
        try:
            for _ in range(250):
                result=await bus.call(Message(destination='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='NameHasOwner',signature='s',body=['org.freedesktop.portal.Desktop']))
                if result.body==[True]:break
                await asyncio.sleep(.04)
            else:raise AssertionError('Real portal frontend did not start')
            response=asyncio.get_running_loop().create_future()
            def signal(message):
                if message.message_type==MessageType.SIGNAL and message.interface=='org.freedesktop.portal.Request' and message.member=='Response' and not response.done():response.set_result(message.body)
            bus.add_message_handler(signal)
            await bus.call(Message(destination='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='AddMatch',signature='s',body=["type='signal',interface='org.freedesktop.portal.Request'"]))
            reply=await bus.call(Message(destination='org.freedesktop.portal.Desktop',path='/org/freedesktop/portal/desktop',interface='org.freedesktop.portal.FileChooser',member='OpenFile',signature='ssa{sv}',body=['','Portal frontend integration',{'handle_token':Variant('s','integration')}]))
            assert reply.message_type==MessageType.METHOD_RETURN,reply.body
            await asyncio.sleep(.6)
            closed=await bus.call(Message(destination='org.freedesktop.portal.Desktop',path=reply.body[0],interface='org.freedesktop.portal.Request',member='Close'))
            assert closed.message_type==MessageType.METHOD_RETURN,closed.body
            # Close is explicitly cancellation; the public request is unexported.
            await asyncio.sleep(.3)
            closed_backend=await bus.call(Message(destination='org.freedesktop.impl.portal.desktop.fltk',path=reply.body[0],interface='org.freedesktop.DBus.Introspectable',member='Introspect'))
            assert closed_backend.message_type==MessageType.ERROR or 'org.freedesktop.impl.portal.Request' not in closed_backend.body[0]
            text=(output/'frontend-private.log').read_text()
            assert 'xdg-desktop-portal-fltk' not in text or backend.poll() is None
            assert backend.poll() is None
            (output/'frontend-test.txt').write_text('PASS real xdg-desktop-portal FileChooser request and cancellation through the FLTK backend\n')
            print((output/'frontend-test.txt').read_text())
        finally:
            portal.terminate();portal.wait(timeout=10)
            backend.terminate();backend.wait(timeout=5)
            bus.disconnect();log.close()

asyncio.run(main())
