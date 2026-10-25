#!/usr/bin/env python3
"""BlueZ protocol fixture on a private bus. Never opens the system bus."""
import asyncio,json,os
from pathlib import Path
from dbus_next import Message, MessageType, Variant
from dbus_next.aio import MessageBus

async def main():
    bus=await MessageBus().connect()
    await bus.request_name('org.bluez')
    await bus.request_name('org.domed.BluetoothFixture')
    adapter='/org/bluez/hci0'; device=adapter+'/dev_11_22_33_44_55_66'
    adapter2='/org/bluez/hci1'; device2=adapter2+'/dev_11_22_33_44_55_77'
    objects={
        adapter:{'org.bluez.Adapter1':{'Alias':Variant('s','Fixture radio 日本語'),'Address':Variant('s','00:11:22:33:44:55'),'Powered':Variant('b',False),'Discoverable':Variant('b',False),'DiscoverableTimeout':Variant('u',0),'Pairable':Variant('b',True),'Discovering':Variant('b',False)}},
        device:{'org.bluez.Device1':{'Alias':Variant('s','Fixture headphones'),'Address':Variant('s','11:22:33:44:55:66'),'Adapter':Variant('o',adapter),'Paired':Variant('b',False),'Connected':Variant('b',False),'Trusted':Variant('b',False),'Blocked':Variant('b',False),'RSSI':Variant('n',-48)},'org.bluez.Battery1':{'Percentage':Variant('y',81)}},
        adapter2:{'org.bluez.Adapter1':{'Alias':Variant('s','Second radio'),'Address':Variant('s','00:11:22:33:44:66'),'Powered':Variant('b',True),'Discoverable':Variant('b',False),'DiscoverableTimeout':Variant('u',180),'Pairable':Variant('b',True),'Discovering':Variant('b',False)}},
        device2:{'org.bluez.Device1':{'Alias':Variant('s','Fixture keyboard'),'Address':Variant('s','11:22:33:44:55:77'),'Adapter':Variant('o',adapter2),'Paired':Variant('b',False),'Connected':Variant('b',False),'Trusted':Variant('b',False),'Blocked':Variant('b',False)}}}
    agent=None;scenario='RequestConfirmation';tasks=set();events=[]
    def log(event):
        events.append(event)
        if os.environ.get('BLUETOOTH_FIXTURE_LOG'):Path(os.environ['BLUETOOTH_FIXTURE_LOG']).write_text(json.dumps(events))
    def changed(path,interface,props):
        objects[path][interface].update(props)
        bus.send(Message.new_signal(path,'org.freedesktop.DBus.Properties','PropertiesChanged','sa{sv}as',[interface,props,[]]))
    def reply(message,signature='',body=[]):bus.send(Message.new_method_return(message,signature,body))
    async def ask(method,signature='',body=[]):
        return await bus.call(Message(destination=agent[0],path=agent[1],interface='org.bluez.Agent1',member=method,signature=signature,body=body))
    async def dispatch(m):
        nonlocal agent,scenario
        try:
            if m.interface=='org.freedesktop.DBus.ObjectManager' and m.member=='GetManagedObjects':reply(m,'a{oa{sa{sv}}}',[objects]);return
            if m.interface=='org.domed.BluetoothFixture':
                if m.member=='Scenario':scenario=m.body[0]
                elif m.member=='Disappear':await bus.release_name('org.bluez')
                elif m.member=='Reappear':await bus.request_name('org.bluez')
                elif m.member=='Agent':reply(m,'ss',list(agent or ('','')));return
                elif m.member=='Cancel':await ask('Cancel')
                elif m.member=='RemoveAdapter':
                    for path in [device2,adapter2]:
                        old=objects.pop(path,None)
                        if old:bus.send(Message.new_signal('/','org.freedesktop.DBus.ObjectManager','InterfacesRemoved','oas',[path,list(old)]))
                elif m.member=='Empty':
                    for path,old in list(objects.items()):
                        del objects[path];bus.send(Message.new_signal('/','org.freedesktop.DBus.ObjectManager','InterfacesRemoved','oas',[path,list(old)]))
                reply(m);return
            if m.interface=='org.bluez.AgentManager1':
                if m.member=='RegisterAgent':agent=(m.sender,m.body[0]);log({'op':'register','capability':m.body[1]})
                reply(m);return
            if m.interface=='org.freedesktop.DBus.Properties' and m.member=='Set':
                iface,key,value=m.body;changed(m.path,iface,{key:value});log({'op':'set','path':m.path,'key':key,'value':value.value});reply(m);return
            if m.interface=='org.bluez.Adapter1':
                if m.member=='StartDiscovery':changed(m.path,m.interface,{'Discovering':Variant('b',True)})
                elif m.member=='StopDiscovery':changed(m.path,m.interface,{'Discovering':Variant('b',False)})
                elif m.member=='RemoveDevice':
                    old=objects.pop(m.body[0]);bus.send(Message.new_signal('/','org.freedesktop.DBus.ObjectManager','InterfacesRemoved','oas',[m.body[0],list(old)]))
                log({'op':m.member,'path':m.path});reply(m);return
            if m.interface=='org.bluez.Device1':
                if m.member=='Pair':
                    mode=scenario
                    if mode in ['RequestConfirmation','DisplayPasskey']:sig='ouq' if mode=='DisplayPasskey' else 'ou';args=[m.path,42]+([3] if mode=='DisplayPasskey' else [])
                    elif mode in ['DisplayPinCode','AuthorizeService']:sig='os';args=[m.path,'1234' if mode=='DisplayPinCode' else '0000110b-0000-1000-8000-00805f9b34fb']
                    else:sig='o';args=[m.path]
                    response=await ask(mode,sig,args)
                    if response.message_type==MessageType.ERROR:
                        log({'op':'rejected','error':response.error_name});bus.send(Message.new_error(m,response.error_name,'Fixture pairing rejected'));return
                    if mode in ['DisplayPasskey','DisplayPinCode']:
                        await asyncio.sleep(.3);await ask('Cancel')
                    log({'op':'paired','mode':mode,'answer':response.body})
                    changed(m.path,m.interface,{'Paired':Variant('b',True)})
                elif m.member=='CancelPairing':await ask('Cancel')
                elif m.member in ['Connect','Disconnect']:changed(m.path,m.interface,{'Connected':Variant('b',m.member=='Connect')})
                log({'op':m.member,'path':m.path});reply(m);return
            bus.send(Message.new_error(m,'org.bluez.Error.NotSupported','Unsupported fixture method'))
        except Exception as e:bus.send(Message.new_error(m,'org.bluez.Error.Failed',str(e)))
    def handler(m):
        if m.message_type!=MessageType.METHOD_CALL or m.interface not in ['org.freedesktop.DBus.ObjectManager','org.freedesktop.DBus.Properties','org.bluez.Adapter1','org.bluez.Device1','org.bluez.AgentManager1','org.domed.BluetoothFixture']:return False
        task=asyncio.create_task(dispatch(m));tasks.add(task);task.add_done_callback(tasks.discard);return True
    bus.add_message_handler(handler)
    Path(os.environ['BLUETOOTH_FIXTURE_READY']).touch()
    await asyncio.Future()
asyncio.run(main())
