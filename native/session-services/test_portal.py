#!/usr/bin/env python3
"""Real D-Bus and FLTK UI tests, on a private bus and Xvfb display only."""
import asyncio
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.parse import unquote

from dbus_next.aio import MessageBus
from dbus_next import Message, MessageType, Variant
from dbus_next.service import ServiceInterface, method, signal
from Xlib import X, display
from Xlib.ext import xtest

BINARY = Path(sys.argv[1]).resolve()
OUT = Path(sys.argv[2]).resolve()
DEST = 'org.freedesktop.impl.portal.desktop.fltk'
OBJECT = '/org/freedesktop/portal/desktop'
disp = display.Display()

class Notifications(ServiceInterface):
    def __init__(self):
        super().__init__('org.freedesktop.Notifications')
        self.received=[]
        self.closed=[]
    @method()
    def GetCapabilities(self)->'as':
        return ['body','actions']
    @method()
    def Notify(self, app:'s', previous:'u', icon:'s', title:'s', body:'s', actions:'as', hints:'a{sv}', timeout:'i')->'u':
        self.received.append([app,title,body,actions,hints])
        return 41
    @method()
    def CloseNotification(self, id:'u'):
        self.closed.append(id)
    @signal()
    def ActionInvoked(self, id:'u', action:'s')->'us':
        return [id,action]

def windows():
    return [w for w in disp.screen().root.query_tree().children
            if w.get_wm_name() == 'DomeD Application Request' and w.get_attributes().map_state == X.IsViewable]

async def window():
    for _ in range(200):
        current = windows()
        if current:
            return current[-1]
        await asyncio.sleep(.03)
    raise AssertionError('No FLTK request dialog appeared')

async def click(w, x, y):
    geom = w.get_geometry()
    xtest.fake_input(disp, X.MotionNotify, x=geom.x+x, y=geom.y+y)
    xtest.fake_input(disp, X.ButtonPress, detail=1)
    xtest.fake_input(disp, X.ButtonRelease, detail=1)
    disp.sync()
    await asyncio.sleep(.15)

async def wait_name(bus):
    for _ in range(200):
        result = await bus.call(Message(destination='org.freedesktop.DBus', path='/org/freedesktop/DBus',
              interface='org.freedesktop.DBus', member='NameHasOwner', signature='s', body=[DEST]))
        if result.body == [True]:
            return
        await asyncio.sleep(.03)
    raise AssertionError('Portal did not acquire its bus name')

async def main():
    assert os.environ.get('DOME_PRIVATE_SERVICE_TEST') == '1'
    assert os.environ.get('DISPLAY') and not os.environ.get('WAYLAND_DISPLAY')
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as folder:
        root = Path(folder)
        (root/'alpha.txt').write_text('fixture')
        (root/'beta.png').write_bytes(b'not a real image')
        log = (OUT/'portal-private.log').open('w')
        process = subprocess.Popen([BINARY], stdout=log, stderr=subprocess.STDOUT)
        bus = await MessageBus().connect()
        await bus.request_name('org.freedesktop.portal.Desktop')
        notifications=Notifications()
        bus.export('/org/freedesktop/Notifications',notifications)
        await bus.request_name('org.freedesktop.Notifications')
        await wait_name(bus)
        counter = 0
        passed = []

        async def request(interface, method, signature, extra, options, cancel=False):
            nonlocal counter
            counter += 1
            handle = f'/org/freedesktop/portal/desktop/request/test/{counter}'
            call = asyncio.create_task(bus.call(Message(destination=DEST, path=OBJECT,
                interface='org.freedesktop.impl.portal.'+interface, member=method,
                signature=signature, body=[handle, 'org.domed.Test', '', *extra, options])))
            w = await window()
            await asyncio.sleep(.4)
            print('Dialog', method, w.get_geometry(), flush=True)
            if method=='ChooseApplication':
                await click(w,120,113)
            await click(w, 500 if cancel else 620, 616)
            try:
                result = await asyncio.wait_for(call, 5)
            except TimeoutError:
                from PIL import Image
                geom=w.get_geometry()
                pic=w.get_image(0,0,geom.width,geom.height,X.ZPixmap,0xffffffff)
                Image.frombytes('RGB',(geom.width,geom.height),pic.data,'raw','BGRX').save(OUT/(method+'-timeout.png'))
                raise
            assert result.message_type == MessageType.METHOD_RETURN, (interface, method, result.body)
            assert result.body[0] == int(cancel), (method, result.body)
            await asyncio.sleep(.1)
            assert not windows(), 'Completed dialog stayed visible'
            return result.body[1]

        folder_option = {'current_folder': Variant('ay', os.fsencode(root)+b'\0')}
        try:
            # Calls from another peer cannot bypass the portal frontend.
            other = await MessageBus().connect()
            result = await other.call(Message(destination=DEST,path=OBJECT,
                interface='org.freedesktop.impl.portal.FileChooser',member='OpenFile',signature='osssa{sv}',
                body=['/org/freedesktop/portal/desktop/request/bad/1','bad.app','','Unauthorized',{}]))
            assert result.message_type == MessageType.ERROR and result.error_name.endswith('AccessDenied')
            other.disconnect(); passed.append('direct backend access denied')

            # Select a real file in the native chooser; inspect its returned URI.
            counter += 1
            handle = f'/org/freedesktop/portal/desktop/request/test/{counter}'
            options = dict(folder_option, filters=Variant('a(sa(us))', [['Text files', [[0, '*.txt']]]]))
            call = asyncio.create_task(bus.call(Message(destination=DEST,path=OBJECT,
                interface='org.freedesktop.impl.portal.FileChooser',member='OpenFile',signature='osssa{sv}',
                body=[handle,'org.domed.Test','','Open fixture',options])))
            w = await window()
            await asyncio.sleep(.5)
            image = w.get_image(0,0,w.get_geometry().width,w.get_geometry().height,X.ZPixmap,0xffffffff)
            from PIL import Image
            Image.frombytes('RGB',(w.get_geometry().width,w.get_geometry().height),image.data,'raw','BGRX').save(OUT/'file-chooser.png')
            await click(w,120,165)  # First file, after the parent-folder row.
            await click(w,620,616)
            result = await asyncio.wait_for(call,5)
            assert result.body[0] == 0, result.body
            assert result.body[1]['uris'].value == [(root/'alpha.txt').as_uri()], result.body
            assert result.body[1]['current_filter'].value == ['Text files', [[0,'*.txt']]]
            passed.append('OpenFile selection and filter')
            await asyncio.sleep(.15)

            result = await request('FileChooser','SaveFile','osssa{sv}',['Save fixture'],
                dict(folder_option,current_name=Variant('s','saved.txt')))
            assert result['uris'].value == [(root/'saved.txt').as_uri()]
            passed.append('SaveFile URI')
            result = await request('FileChooser','SaveFiles','osssa{sv}',['Save fixtures'],
                dict(folder_option,files=Variant('aay',[b'alpha.txt\0',b'new.txt\0'])))
            assert [unquote(uri) for uri in result['uris'].value] == ['file://'+str(root/'alpha (1).txt'),'file://'+str(root/'new.txt')], result
            passed.append('SaveFiles collision avoidance')

            # Request.Close must cancel and complete the outstanding method.
            counter += 1; handle=f'/org/freedesktop/portal/desktop/request/test/{counter}'
            call=asyncio.create_task(bus.call(Message(destination=DEST,path=OBJECT,
                interface='org.freedesktop.impl.portal.FileChooser',member='OpenFile',signature='osssa{sv}',
                body=[handle,'org.domed.Test','','Close fixture',folder_option])))
            await window()
            closed=await bus.call(Message(destination=DEST,path=handle,
                interface='org.freedesktop.impl.portal.Request',member='Close'))
            assert closed.message_type == MessageType.METHOD_RETURN
            assert (await asyncio.wait_for(call,5)).body[0] == 1
            passed.append('Request.Close cancels dialog')
            await asyncio.sleep(.15)

            result=await request('Access','AccessDialog','osssssa{sv}',['Microphone access','Test application','Allow microphone access?'],
                {'choices':Variant('a(ssa(ss)s)',[['remember','Remember this decision',[],'false']])})
            assert result['choices'].value == [['remember','false']]
            passed.append('explicit permission consent and choices')
            await request('Access','AccessDialog','osssssa{sv}',['Permission','Test','Cancel access'],{},cancel=True)
            passed.append('permission cancellation')
            result=await request('AppChooser','ChooseApplication','ossasa{sv}',[['org.domed.FLTerm.desktop']],{})
            assert result['choice'].value=='org.domed.FLTerm.desktop'
            passed.append('application chooser selection')
            await request('Print','PreparePrint','osssa{sv}a{sv}a{sv}',['Print fixture',{},{}],{},cancel=True)
            passed.append('print dialog and cancellation')
            await request('Account','GetUserInformation','ossa{sv}',[],{},cancel=True)
            passed.append('account disclosure cancellation')
            await request('Email','ComposeEmail','ossa{sv}',[],{},cancel=True)
            passed.append('email composition cancellation')
            import io
            from PIL import Image
            icon=io.BytesIO();Image.new('RGB',(16,16),'#6babff').save(icon,format='PNG')
            result=await request('DynamicLauncher','PrepareInstall','osssva{sv}',
                ['Fixture launcher',Variant('(sv)',['bytes',Variant('ay',icon.getvalue())])],{})
            assert result['name'].value=='Fixture launcher'
            passed.append('launcher icon preview and explicit consent')

            actions=[]
            def notice_signal(message):
                if message.message_type==MessageType.SIGNAL and message.interface=='org.freedesktop.impl.portal.Notification' and message.member=='ActionInvoked':actions.append(message.body)
            bus.add_message_handler(notice_signal)
            await bus.call(Message(destination='org.freedesktop.DBus',path='/org/freedesktop/DBus',interface='org.freedesktop.DBus',member='AddMatch',signature='s',body=["type='signal',interface='org.freedesktop.impl.portal.Notification'"]))
            result=await bus.call(Message(destination=DEST,path=OBJECT,interface='org.freedesktop.impl.portal.Notification',member='AddNotification',signature='ssa{sv}',body=['org.domed.Test','fixture',{'title':Variant('s','Fixture notification'),'body':Variant('s','Fixture body'),'default-action':Variant('s','app.open'),'default-action-target':Variant('s','fixture-target')}]))
            assert result.message_type==MessageType.METHOD_RETURN,result.body
            assert notifications.received[-1][0]=='org.domed.Test'
            assert notifications.received[-1][2]=='Fixture body'
            notifications.ActionInvoked(41,'default')
            await asyncio.sleep(.25)
            assert actions and actions[-1][:3]==['org.domed.Test','fixture','app.open'],actions
            assert actions[-1][3][0].value=='fixture-target'
            await bus.call(Message(destination=DEST,path=OBJECT,interface='org.freedesktop.impl.portal.Notification',member='RemoveNotification',signature='ss',body=['org.domed.Test','fixture']))
            await asyncio.sleep(.2)
            assert notifications.closed==[41]
            passed.append('notification forwarding, actions and removal')

            monitor='/org/freedesktop/portal/desktop/session/test/monitor'
            result=await bus.call(Message(destination=DEST,path=OBJECT,interface='org.freedesktop.impl.portal.Inhibit',member='CreateMonitor',signature='ooss',body=['/org/freedesktop/portal/desktop/request/test/monitor',monitor,'org.domed.Test','']))
            assert result.body==[0],result.body
            result=await bus.call(Message(destination=DEST,path=monitor,interface='org.freedesktop.impl.portal.Session',member='Close'))
            assert result.message_type==MessageType.METHOD_RETURN,result.body
            passed.append('session monitor lifecycle')
            # Frontend disappearance releases every pending request.
            counter += 1; handle=f'/org/freedesktop/portal/desktop/request/test/{counter}'
            call=asyncio.create_task(bus.call(Message(destination=DEST,path=OBJECT,
                interface='org.freedesktop.impl.portal.FileChooser',member='OpenFile',signature='osssa{sv}',
                body=[handle,'org.domed.Test','','Owner loss',{}])))
            await window(); await bus.release_name('org.freedesktop.portal.Desktop')
            assert (await asyncio.wait_for(call,5)).body[0] == 1
            await asyncio.sleep(.15);assert not windows()
            passed.append('frontend disappearance cancels requests')
        finally:
            process.terminate(); process.wait(timeout=5); bus.disconnect();log.close()
        content=(OUT/'portal-private.log').read_text()
        assert 'CRITICAL' not in content and 'ERROR' not in content, content
        (OUT/'portal-tests.json').write_text(json.dumps({'passed':passed},indent=2))
        print(json.dumps({'passed':passed},indent=2))

asyncio.run(main())
