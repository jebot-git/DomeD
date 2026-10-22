#!/usr/bin/env python3
"""Private-bus service fixtures. No real device, IM or wallet is touched."""
import asyncio,os,json
from pathlib import Path
from dbus_next.aio import MessageBus
from dbus_next import Variant,PropertyAccess
from dbus_next.service import ServiceInterface,method,dbus_property
class Daemon(ServiceInterface):
 def __init__(self):super().__init__('org.kde.kdeconnect.daemon')
 @method()
 def devices(self,reachable:'b',paired:'b')->'as':return ['test_phone']
 @method()
 def forceOnNetworkChange(self):pass
 @method()
 def setAnnouncedName(self,name:'s'):pass
class Device(ServiceInterface):
 def __init__(self):super().__init__('org.kde.kdeconnect.device')
 @dbus_property(name='name',access=PropertyAccess.READ)
 def device_name(self)->'s':return 'Fixture Phone'
 @dbus_property(access=PropertyAccess.READ)
 def isReachable(self)->'b':return True
 @dbus_property(access=PropertyAccess.READ)
 def isPaired(self)->'b':return True
 @dbus_property(access=PropertyAccess.READ)
 def verificationKey(self)->'s':return 'TEST-KEY'
 @method()
 def loadedPlugins(self)->'as':return ['kdeconnect_ping','kdeconnect_share','kdeconnect_findmyphone']
 @method()
 def requestPairing(self):pass
 @method()
 def acceptPairing(self):pass
 @method()
 def cancelPairing(self):pass
 @method()
 def unpair(self):pass
 @method()
 def setPluginEnabled(self,name:'s',enabled:'b'):pass
class Battery(ServiceInterface):
 def __init__(self):super().__init__('org.kde.kdeconnect.device.battery')
 @dbus_property(access=PropertyAccess.READ)
 def charge(self)->'i':return 75
class Ping(ServiceInterface):
 def __init__(self):super().__init__('org.kde.kdeconnect.device.ping')
 @method()
 def sendPing(self,message:'s'):pass
class Find(ServiceInterface):
 def __init__(self):super().__init__('org.kde.kdeconnect.device.findmyphone')
 @method()
 def ring(self):pass
class Share(ServiceInterface):
 def __init__(self):super().__init__('org.kde.kdeconnect.device.share')
 @method()
 def shareUrl(self,url:'s'):pass
 @method()
 def shareText(self,text:'s'):pass
class Input(ServiceInterface):
 def __init__(self):
  super().__init__('org.fcitx.Fcitx.Controller1');self.current='keyboard-us';self.group='Default';self.layout='us';self.items=[['keyboard-us',''],['anthy','']];self.raw={'Behavior':Variant('a{sv}',{'ActiveByDefault':Variant('s','False'),'Keep':Variant('s','preserved')})}
 @method()
 def AvailableInputMethods(self)->'a(ssssssb)':return [['keyboard-us','English','English','input-keyboard','US','en',False],['anthy','Japanese','日本語','anthy','あ','ja',True]]
 @method()
 def CurrentInputMethod(self)->'s':return self.current
 @method()
 def CurrentInputMethodGroup(self)->'s':return self.group
 @method()
 def InputMethodGroups(self)->'as':return ['Default','Writing']
 @method()
 def InputMethodGroupInfo(self,group:'s')->'sa(ss)':return [self.layout,self.items]
 @method()
 def SetInputMethodGroupInfo(self,group:'s',layout:'s',items:'a(ss)'):self.layout=layout;self.items=items
 @method()
 def SetCurrentIM(self,name:'s'):self.current=name
 @method()
 def SwitchInputMethodGroup(self,name:'s'):self.group=name
 @method()
 def AddInputMethodGroup(self,name:'s'):pass
 @method()
 def RemoveInputMethodGroup(self,name:'s'):pass
 @method()
 def Toggle(self):pass
 @method()
 def Activate(self):pass
 @method()
 def Deactivate(self):pass
 @method()
 def ReloadConfig(self):pass
 @method()
 def Save(self):pass
 @method()
 def GetAddons(self)->'a(sssibb)':return [['classicui','Classic UI','',4,True,True]]
 @method()
 def GetConfig(self,uri:'s')->'va(sa(sssva{sv}))':
  if os.environ.get('TRAY_FIXTURE_CONFIG_REQUESTS'):
   with open(os.environ['TRAY_FIXTURE_CONFIG_REQUESTS'],'a') as log:log.write(uri+'\n')
  return [Variant('a{sv}',self.raw),[]]
 @method()
 def SetConfig(self,uri:'s',data:'v'):self.raw=data.value
class Wallet(ServiceInterface):
 def __init__(self):super().__init__('org.kde.KWallet');self.data={'Passwords':{'test':'disposable-secret'}};self.types={}
 def check(self,handle,app):assert handle==7 and app=='DomeD Wallet Manager'
 @method()
 def wallets(self)->'as':return ['Fixture']
 @method()
 def open(self,name:'s',window:'x',app:'s')->'i':assert name=='Fixture';return 7
 @method()
 def folderList(self,handle:'i',app:'s')->'as':self.check(handle,app);return list(self.data)
 @method()
 def entryList(self,handle:'i',folder:'s',app:'s')->'as':self.check(handle,app);return list(self.data[folder])
 @method()
 def entryType(self,handle:'i',folder:'s',key:'s',app:'s')->'i':self.check(handle,app);return self.types.get((folder,key),1)
 @method()
 def readPassword(self,handle:'i',folder:'s',key:'s',app:'s')->'s':self.check(handle,app);return self.data[folder][key]
 @method()
 def writePassword(self,handle:'i',folder:'s',key:'s',value:'s',app:'s')->'i':self.check(handle,app);self.data[folder][key]=value;return 0
 @method()
 def readMap(self,handle:'i',folder:'s',key:'s',app:'s')->'ay':self.check(handle,app);return self.data[folder][key]
 @method()
 def readEntry(self,handle:'i',folder:'s',key:'s',app:'s')->'ay':self.check(handle,app);return self.data[folder][key]
 @method()
 def writeMap(self,handle:'i',folder:'s',key:'s',value:'ay',app:'s')->'i':self.check(handle,app);self.data[folder][key]=value;self.types[(folder,key)]=3;return 0
 @method()
 def writeEntry(self,handle:'i',folder:'s',key:'s',value:'ay',type:'i',app:'s')->'i':self.check(handle,app);assert type==2;self.data[folder][key]=value;self.types[(folder,key)]=type;return 0
 @method()
 def renameEntry(self,handle:'i',folder:'s',key:'s',new:'s',app:'s')->'i':self.check(handle,app);self.data[folder][new]=self.data[folder].pop(key);return 0
 @method()
 def removeEntry(self,handle:'i',folder:'s',key:'s',app:'s')->'i':self.check(handle,app);del self.data[folder][key];return 0
 @method()
 def createFolder(self,handle:'i',folder:'s',app:'s')->'b':self.check(handle,app);self.data[folder]={};return True
 @method()
 def removeFolder(self,handle:'i',folder:'s',app:'s')->'b':self.check(handle,app);del self.data[folder];return True
 @method()
 def close(self,handle:'i',force:'b',app:'s')->'i':self.check(handle,app);assert not force;return 0
class Watcher(ServiceInterface):
 def __init__(self):super().__init__('org.kde.StatusNotifierWatcher');self.items=['org.domed.Fixture/Item','org.domed.Fixture/Fcitx']
 @method()
 def RegisterStatusNotifierItem(self,service:'s'):self.items.append('org.domed.Tray'+service if service.startswith('/') else service)
 @dbus_property(access=PropertyAccess.READ)
 def RegisteredStatusNotifierItems(self)->'as':return self.items
class Item(ServiceInterface):
 def __init__(self):super().__init__('org.kde.StatusNotifierItem')
 @dbus_property(access=PropertyAccess.READ)
 def Title(self)->'s':return 'Fixture Tray'
 @dbus_property(access=PropertyAccess.READ)
 def Id(self)->'s':return 'fixture'
 @dbus_property(access=PropertyAccess.READ)
 def Status(self)->'s':return 'Active'
 @dbus_property(access=PropertyAccess.READ)
 def Menu(self)->'o':return '/Menu'
class Menu(ServiceInterface):
 def __init__(self):super().__init__('com.canonical.dbusmenu')
 @method()
 def AboutToShow(self,parent:'i')->'b':return False
 @method()
 def GetLayout(self,parent:'i',depth:'i',properties:'as')->'u(ia{sv}av)':return [1,[0,{},[Variant('(ia{sv}av)',[10,{'label':Variant('s','Do _something'),'enabled':Variant('b',True),'toggle-type':Variant('s','checkmark'),'toggle-state':Variant('i',1)},[]])]]]
 @method()
 def Event(self,id:'i',event:'s',value:'v',timestamp:'u'):assert id==10 and event=='clicked'
class FcitxItem(ServiceInterface):
 def __init__(self):super().__init__('org.kde.StatusNotifierItem')
 @dbus_property(access=PropertyAccess.READ)
 def Id(self)->'s':return 'fcitx5-language'
 @dbus_property(access=PropertyAccess.READ)
 def Title(self)->'s':return 'Fcitx5 language indicator'
 @dbus_property(access=PropertyAccess.READ)
 def Status(self)->'s':return 'Active'
 @dbus_property(access=PropertyAccess.READ)
 def IconPixmap(self)->'a(iiay)':return [[16,16,bytes([255,255,0,255])*256]]
async def main():
 bus=await MessageBus().connect()
 for name in ['org.kde.kdeconnect','org.fcitx.Fcitx5','org.kde.kwalletd6','org.kde.StatusNotifierWatcher','org.domed.Fixture']:await bus.request_name(name)
 for path,service in [('/modules/kdeconnect',Daemon()),('/modules/kdeconnect/devices/test_phone',Device()),('/modules/kdeconnect/devices/test_phone/battery',Battery()),('/modules/kdeconnect/devices/test_phone/ping',Ping()),('/modules/kdeconnect/devices/test_phone/findmyphone',Find()),('/modules/kdeconnect/devices/test_phone/share',Share()),('/controller',Input()),('/modules/kwalletd6',Wallet()),('/StatusNotifierWatcher',Watcher()),('/Item',Item()),('/Menu',Menu())]:bus.export(path,service)
 bus.export('/Fcitx',FcitxItem())
 Path(os.environ['TRAY_FIXTURE_READY']).touch();await asyncio.Event().wait()
asyncio.run(main())
