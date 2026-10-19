#!/usr/bin/env python3
"""Private D-Bus test services. Records requests; never performs session operations."""
import asyncio,json,sys
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface,method,dbus_property
from dbus_next.constants import PropertyAccess
log=open(sys.argv[1],'w',buffering=1)
def record(kind,**fields):log.write(json.dumps({'kind':kind,**fields})+'\n')
class Locker(ServiceInterface):
    def __init__(self):super().__init__('org.freedesktop.ScreenSaver')
    @method()
    def Lock(self):record('lock')
class Login(ServiceInterface):
    def __init__(self):super().__init__('org.freedesktop.login1.Manager')
    @method()
    def CanSuspend(self)->'s':return 'yes'
    @method()
    def CanReboot(self)->'s':return 'yes'
    @method()
    def CanPowerOff(self)->'s':return 'yes'
    @method()
    def Suspend(self,interactive:'b'):record('suspend',interactive=interactive)
    @method()
    def Reboot(self,interactive:'b'):record('restart',interactive=interactive)
    @method()
    def PowerOff(self,interactive:'b'):record('shutdown',interactive=interactive)
class Seat(ServiceInterface):
    def __init__(self):super().__init__('org.freedesktop.DisplayManager.Seat')
    @dbus_property(access=PropertyAccess.READ)
    def CanSwitch(self)->'b':return True
    @method()
    def SwitchToGreeter(self):record('switch_user')
async def main():
    bus=await MessageBus().connect()
    bus.export('/ScreenSaver',Locker());bus.export('/org/freedesktop/ScreenSaver',Locker());await bus.request_name('org.freedesktop.ScreenSaver')
    bus.export('/org/freedesktop/login1',Login());await bus.request_name('org.freedesktop.login1')
    bus.export('/org/freedesktop/DisplayManager/Seat0',Seat());await bus.request_name('org.freedesktop.DisplayManager')
    record('ready');await asyncio.Future()
asyncio.run(main())
