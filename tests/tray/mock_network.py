#!/usr/bin/env python3
"""NetworkManager D-Bus fixture; exclusively uses the supplied private session bus."""
import asyncio, json, os
from pathlib import Path
from dbus_next import Message, MessageType, Variant
from dbus_next.aio import MessageBus

NM = 'org.freedesktop.NetworkManager'
ROOT = '/org/freedesktop/NetworkManager'
WIFI, WIRED = ROOT+'/Devices/1', ROOT+'/Devices/2'
AP, PROFILE = ROOT+'/AccessPoint/1', ROOT+'/Settings/1'
UUID = '226c1638-0e3e-4b62-9a58-d1840ef64e16'

async def main():
    assert os.environ.get('TRAY_TEST_PRIVATE_BUS') == '1'
    bus = await MessageBus().connect()
    objects, settings, events, tasks = {}, {}, [], set()
    def props(values):return {k:Variant(signature,value) for k,(signature,value) in values.items()}
    def send(message):bus.send(message)
    def signal(path, iface, member, signature, body):send(Message.new_signal(path,iface,member,signature,body))
    def changed(path, iface, values):
        if iface==NM+'.Device' and 'State' in values:
            values={**values,'StateReason':Variant('(uu)',[values['State'].value,0])}
        objects[path][iface].update(values)
        signal(path,'org.freedesktop.DBus.Properties','PropertiesChanged','sa{sv}as',[iface,values,[]])
    def added(path, interfaces):
        objects[path] = interfaces
        signal('/org/freedesktop','org.freedesktop.DBus.ObjectManager','InterfacesAdded','oa{sa{sv}}',[path,interfaces])
    def removed(path):
        old=objects.pop(path)
        signal('/org/freedesktop','org.freedesktop.DBus.ObjectManager','InterfacesRemoved','oas',[path,list(old)])
    def log(event):
        events.append(event)
        if os.environ.get('NETWORK_FIXTURE_LOG'):Path(os.environ['NETWORK_FIXTURE_LOG']).write_text(json.dumps(events))
    objects[ROOT]={NM:props({'Version':('s','1.56.1'),'State':('u',70),'NetworkingEnabled':('b',True),'WirelessEnabled':('b',True),'WirelessHardwareEnabled':('b',True),'Devices':('ao',[WIFI,WIRED]),'AllDevices':('ao',[WIFI,WIRED]),'ActiveConnections':('ao',[]),'Startup':('b',False)})}
    objects[ROOT+'/Settings']={NM+'.Settings':props({'Connections':('ao',[PROFILE]),'CanModify':('b',True),'Hostname':('s','fixture')})}
    for path, kind, name in [(WIFI,2,'wlan-test'),(WIRED,1,'eth-test')]:
        objects[path]={NM+'.Device':props({'DeviceType':('u',kind),'Interface':('s',name),'IpInterface':('s',name),'State':('u',30),'StateReason':('(uu)',[30,0]),'Managed':('b',True),'Real':('b',True),'ActiveConnection':('o','/'),'AvailableConnections':('ao',[PROFILE] if kind==2 else []),'Ip4Config':('o',ROOT+'/IP4Config/1' if kind==2 else '/'),'Ip6Config':('o','/')})}
    objects[WIFI][NM+'.Device.Wireless']=props({'AccessPoints':('ao',[AP]),'ActiveAccessPoint':('o','/'),'Mode':('u',2),'WirelessCapabilities':('u',0x7ff),'HwAddress':('s','02:00:00:00:00:01')})
    objects[WIRED][NM+'.Device.Wired']=props({'Carrier':('b',True)})
    objects[AP]={NM+'.AccessPoint':props({'Flags':('u',1),'RsnFlags':('u',0x100),'WpaFlags':('u',0),'Ssid':('ay','Česká síť'.encode()),'Strength':('y',82),'Mode':('u',2),'Frequency':('u',2412),'HwAddress':('s','02:00:00:00:00:02')})}
    objects[ROOT+'/IP4Config/1']={NM+'.IP4Config':props({'AddressData':('aa{sv}',[props({'address':('s','192.0.2.10'),'prefix':('u',24)})]),'NameserverData':('aa{sv}',[props({'address':('s','192.0.2.1')})])})}
    objects[PROFILE]={NM+'.Settings.Connection':props({'Unsaved':('b',False),'Flags':('u',0)})}
    settings[PROFILE]={
        'connection':props({'id':('s','Saved Wi-Fi'),'uuid':('s',UUID),'type':('s','802-11-wireless'),'autoconnect':('b',True)}),
        '802-11-wireless':props({'ssid':('ay','Česká síť'.encode()),'mode':('s','infrastructure')}),
        '802-11-wireless-security':props({'key-mgmt':('s','wpa-psk')}),
        'ipv4':props({'method':('s','auto')}), 'ipv6':props({'method':('s','auto')})}
    deny=False
    def disconnect():
        changed(ROOT,NM,{'ActiveConnections':Variant('ao',[])})
        for device in [WIFI,WIRED]:
            if device in objects:changed(device,NM+'.Device',{'ActiveConnection':Variant('o','/'),'State':Variant('u',30)})
        for path in list(objects):
            if NM+'.Connection.Active' in objects[path]:removed(path)
    def activate(profile, device, specific):
        disconnect()
        active=ROOT+'/ActiveConnection/'+str(len(events)+1)
        fields=settings[profile]['connection']
        added(active,{NM+'.Connection.Active':props({'Connection':('o',profile),'SpecificObject':('o',specific),'Id':('s',fields['id'].value),'Uuid':('s',fields['uuid'].value),'Type':('s',fields['type'].value),'Devices':('ao',[device]),'State':('u',2),'Default':('b',True),'Vpn':('b',False)})})
        changed(ROOT,NM,{'ActiveConnections':Variant('ao',[active])})
        changed(device,NM+'.Device',{'ActiveConnection':Variant('o',active),'State':Variant('u',100)})
        return active
    async def dispatch(m):
        nonlocal deny
        def reply(signature='',body=[]):send(Message.new_method_return(m,signature,body))
        try:
            if m.interface=='org.domed.NetworkFixture':
                if m.member=='Deny':deny=m.body[0]
                elif m.member=='Disappear':await bus.release_name(NM)
                elif m.member=='Reappear':await bus.request_name(NM)
                elif m.member=='RemoveDevice':
                    removed(WIFI);changed(ROOT,NM,{'Devices':Variant('ao',[WIRED]),'AllDevices':Variant('ao',[WIRED])})
                elif m.member=='Events':reply('s',[json.dumps(events)]);return
                reply();return
            if m.interface=='org.freedesktop.DBus.ObjectManager':reply('a{oa{sa{sv}}}',[objects]);return
            if m.interface=='org.freedesktop.DBus.Properties':
                if m.member=='GetAll':reply('a{sv}',[objects.get(m.path,{}).get(m.body[0],{})]);return
                if m.member=='Get':reply('v',[objects[m.path][m.body[0]][m.body[1]]]);return
            if m.member=='GetSettings':reply('a{sa{sv}}',[settings[m.path]]);return
            if m.member=='GetPermissions':reply('a{ss}',[{}]);return
            if deny:send(Message.new_error(m,NM+'.PermissionDenied','Fixture policy denied this operation'));return
            if m.interface=='org.freedesktop.DBus.Properties' and m.member=='Set':
                iface,key,value=m.body;changed(m.path,iface,{key:value});log({'op':'set','key':key,'value':value.value});reply();return
            log({'op':m.member,'path':m.path})
            if m.member=='Enable':changed(ROOT,NM,{'NetworkingEnabled':Variant('b',m.body[0])});reply()
            elif m.member=='ActivateConnection':reply('o',[activate(m.body[0],WIFI if m.body[1]=='/' else m.body[1],m.body[2])])
            elif m.member=='AddAndActivateConnection':
                new=ROOT+'/Settings/'+str(len(settings)+1)
                settings[new]=m.body[0]
                added(new,{NM+'.Settings.Connection':props({'Unsaved':('b',False),'Flags':('u',0)})})
                changed(ROOT+'/Settings',NM+'.Settings',{'Connections':Variant('ao',list(settings))})
                reply('oo',[new,activate(new,m.body[1],m.body[2])])
            elif m.member=='Update2':
                settings[m.path]=m.body[0];signal(m.path,NM+'.Settings.Connection','Updated','',[]);reply('a{sv}',[{}])
            elif m.member=='Delete':
                settings.pop(m.path);removed(m.path)
                changed(ROOT+'/Settings',NM+'.Settings',{'Connections':Variant('ao',list(settings))});reply()
            elif m.member in ['Disconnect','DeactivateConnection']:
                disconnect();reply()
            elif m.member=='RequestScan':reply()
            else:send(Message.new_error(m,NM+'.NotSupported','Unsupported fixture method '+m.member))
        except Exception as e:send(Message.new_error(m,NM+'.Failed',str(e)))
    def handler(m):
        if m.message_type!=MessageType.METHOD_CALL or not (m.interface or '').startswith(('org.freedesktop.NetworkManager','org.freedesktop.DBus.Properties','org.freedesktop.DBus.ObjectManager','org.domed.NetworkFixture')):return False
        task=asyncio.create_task(dispatch(m));tasks.add(task);task.add_done_callback(tasks.discard);return True
    bus.add_message_handler(handler)
    await bus.request_name(NM)
    await bus.request_name('org.domed.NetworkFixture')
    Path(os.environ['NETWORK_FIXTURE_READY']).touch()
    await asyncio.Future()

if __name__=='__main__':asyncio.run(main())
