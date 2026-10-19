#!/usr/bin/env python3
"""Disposable StatusNotifierItem/DBusMenu fixture; uses only the test's private bus."""
import asyncio,json,sys
from dbus_next import Variant
from dbus_next.aio import MessageBus
from dbus_next.service import ServiceInterface,method,dbus_property
from dbus_next.constants import PropertyAccess
journal=open(sys.argv[1],'w',buffering=1)
def record(kind,**fields):journal.write(json.dumps({'kind':kind,**fields})+'\n')
class Item(ServiceInterface):
 def __init__(self):super().__init__('org.kde.StatusNotifierItem')
 @dbus_property(access=PropertyAccess.READ)
 def Category(self)->'s':return 'ApplicationStatus'
 @dbus_property(access=PropertyAccess.READ)
 def Id(self)->'s':return 'dome-tray-fixture'
 @dbus_property(access=PropertyAccess.READ)
 def Title(self)->'s':return 'Dome Test Tray'
 @dbus_property(access=PropertyAccess.READ)
 def Status(self)->'s':return 'Active'
 @dbus_property(access=PropertyAccess.READ)
 def IconName(self)->'s':return ''
 @dbus_property(access=PropertyAccess.READ)
 def IconPixmap(self)->'a(iiay)':return [[22,22,bytes([255,20,20,20])*22*22]]
 @dbus_property(access=PropertyAccess.READ)
 def ItemIsMenu(self)->'b':return True
 @dbus_property(access=PropertyAccess.READ)
 def Menu(self)->'o':return '/Menu'
 @method()
 def Activate(self,x:'i',y:'i'):record('activate',x=x,y=y)
 @method()
 def ContextMenu(self,x:'i',y:'i'):record('context',x=x,y=y)
class Menu(ServiceInterface):
 def __init__(self):super().__init__('com.canonical.dbusmenu')
 @method()
 def AboutToShow(self,id:'i')->'b':return False
 @method()
 def GetLayout(self,parent:'i',depth:'i',properties:'as')->'u(ia{sv}av)':
  record('layout')
  return [1,[0,{},[Variant('(ia{sv}av)',[10,{'label':Variant('s','Tray Test Action'),'enabled':Variant('b',True)},[]]),Variant('(ia{sv}av)',[11,{'label':Variant('s','Unavailable'),'enabled':Variant('b',False)},[]])]]]
 @method()
 def Event(self,id:'i',event:'s',data:'v',timestamp:'u'):record('event',id=id,event=event)
async def main():
 bus=await MessageBus().connect();await bus.request_name('org.example.DomeTray');bus.export('/StatusNotifierItem',Item());bus.export('/Menu',Menu())
 for _ in range(100):
  try:
   introspection=await bus.introspect('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher');watcher=bus.get_proxy_object('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher',introspection).get_interface('org.kde.StatusNotifierWatcher');await watcher.call_register_status_notifier_item('org.example.DomeTray');record('registered');break
  except Exception:await asyncio.sleep(.05)
 else:raise RuntimeError('Watcher did not start')
 await asyncio.Event().wait()
asyncio.run(main())
