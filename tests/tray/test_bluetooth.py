#!/usr/bin/env python3
"""BlueZ integration regression: private bus only, no physical radio access."""
import asyncio,json,os,signal,subprocess,sys,tempfile,time
from pathlib import Path
from dbus_next import Message,MessageType
from dbus_next.aio import MessageBus
signal.pthread_sigmask(signal.SIG_SETMASK,[])
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1','Run through dbus-run-session with TRAY_TEST_PRIVATE_BUS=1'
ROOT=Path(__file__).resolve().parents[2]
BUILD=Path(os.environ.get('DOME_TRAY_BUILD_DIR',ROOT/'build/tray'))
A='/org/bluez/hci0';D=A+'/dev_11_22_33_44_55_66';A2='/org/bluez/hci1';D2=A2+'/dev_11_22_33_44_55_77'
def wait(fn):
    end=time.monotonic()+12
    while time.monotonic()<end:
        value=fn()
        if value:return value
        time.sleep(.04)
    raise AssertionError('Bluetooth test timed out')
with tempfile.TemporaryDirectory(prefix='dome-bluetooth-') as directory:
    root=Path(directory);ready=root/'ready';log=root/'events.json'
    env={**os.environ,'DBUS_SYSTEM_BUS_ADDRESS':os.environ['DBUS_SESSION_BUS_ADDRESS'],'BLUETOOTH_FIXTURE_READY':str(ready),'BLUETOOTH_FIXTURE_LOG':str(log)}
    fixture=subprocess.Popen([sys.executable,str(ROOT/'tests/tray/mock_bluetooth.py')],env=env)
    driver=None
    try:
        wait(ready.exists)
        driver=subprocess.Popen([str(BUILD/'bluetooth-service-driver')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env=env)
        def call(op='state',**args):
            driver.stdin.write(json.dumps({'op':op,**args})+'\n');driver.stdin.flush()
            line=driver.stdout.readline();assert line,'Driver exited unexpectedly'
            return json.loads(line)
        def state():return call()
        def device(path=D):return next((d for d in state()['devices'] if d['path']==path),{})
        def adapter(path=A):return next((a for a in state()['adapters'] if a['path']==path),{})
        def action(op,path=D):call('action',path=path,action=op)
        async def fixture_call(method,signature='',body=[]):
            bus=await MessageBus().connect()
            try:
                r=await bus.call(Message(destination='org.domed.BluetoothFixture',path='/fixture',interface='org.domed.BluetoothFixture',member=method,signature=signature,body=body))
                assert r.message_type!=MessageType.ERROR,r.body
                return r.body
            finally:bus.disconnect()
        def control(method,signature='',body=[]):return asyncio.run(fixture_call(method,signature,body))
        wait(lambda:len(state()['adapters'])==2 and state()['agent'])
        assert device()['battery']==81 and device()['RSSI']==-48
        assert '日本語' in adapter()['Alias']
        action('power-on',A);wait(lambda:adapter()['Powered'])
        action('visible-on',A);wait(lambda:adapter()['Discoverable'])
        events=json.loads(log.read_text());assert next(e for e in events if e.get('key')=='DiscoverableTimeout')['value']==180
        action('scan',A);wait(lambda:adapter()['scanning'])
        call('stop');wait(lambda:not adapter()['scanning'])
        # Authenticate the caller of Agent1; an arbitrary bus peer cannot prompt.
        dest,path=control('Agent')
        async def forged():
            bus=await MessageBus().connect()
            try:return await bus.call(Message(destination=dest,path=path,interface='org.bluez.Agent1',member='RequestConfirmation',signature='ou',body=[D,42]))
            finally:bus.disconnect()
        assert asyncio.run(forged()).error_name=='org.bluez.Error.Rejected'
        assert state()['prompt'] is None
        def prompt():return state()['prompt']
        action('pair');p=wait(prompt)
        assert p['kind']=='RequestConfirmation' and p['code']=='000042'
        call('answer',id=p['id']+1,accept=True);assert prompt()['id']==p['id']
        call('answer',id=p['id'],accept=False)
        wait(lambda:not state()['busy']);assert not device()['Paired']
        action('pair');p=wait(prompt)
        call('answer',id=p['id'],accept=True);wait(lambda:device()['Paired'] and not state()['busy'])
        assert not device()['Trusted'],'Pairing must not silently trust the device'
        for op,key,value in [('connect','Connected',True),('disconnect','Connected',False),('trust','Trusted',True),('untrust','Trusted',False),('block','Blocked',True),('unblock','Blocked',False)]:
            action(op);wait(lambda:device()[key]==value and not state()['busy'])
        for mode,text in [('RequestPasskey','000007'),('RequestPinCode','abcd1234'),('RequestAuthorization',''),('AuthorizeService','')]:
            control('Scenario','s',[mode]);action('pair');p=wait(prompt);assert p['kind']==mode
            if mode=='RequestPasskey':
                call('answer',id=p['id'],accept=True,value='12x');wait(lambda:'numeric' in state()['error']);assert prompt()['id']==p['id']
            call('answer',id=p['id'],accept=True,value=text);wait(lambda:not state()['busy'])
        events=json.loads(log.read_text())
        assert any(e.get('mode')=='RequestPasskey' and e['answer']==[7] for e in events)
        assert any(e.get('mode')=='RequestPinCode' and e['answer']==['abcd1234'] for e in events)
        for mode in ['DisplayPasskey','DisplayPinCode']:
            control('Scenario','s',[mode]);action('pair');p=wait(prompt)
            assert p['code']==('000042' if mode=='DisplayPasskey' else '1234')
            if mode=='DisplayPasskey':assert p['entered']==3
            wait(lambda:not state()['busy']);assert prompt() is None
        control('Scenario','s',['RequestConfirmation']);action('pair');p=wait(prompt)
        # An outstanding Pair must not prevent cancellation or other adapter work.
        action('scan',A2);wait(lambda:adapter(A2)['scanning'])
        action('cancel');wait(lambda:not state()['busy'] and prompt() is None)
        call('stop');wait(lambda:not adapter(A2)['scanning'])
        action('pair');wait(prompt);call('cancel');wait(lambda:not state()['busy'] and prompt() is None)
        action('pair');wait(prompt);control('Disappear')
        wait(lambda:not state()['available']);assert prompt() is None and not state()['devices']
        control('Reappear');wait(lambda:state()['available'] and state()['agent'])
        action('scan',A2);wait(lambda:adapter(A2)['scanning'])
        control('RemoveAdapter');wait(lambda:len(state()['adapters'])==1)
        action('remove');wait(lambda:not state()['devices'])
        action('connect');wait(lambda:'no longer available' in state()['error'])
        control('Empty');wait(lambda:not state()['adapters']);assert state()['available']
        driver.stdin.close();driver.wait(timeout=5);assert driver.returncode==0
        print('PASS: adapters, bounded visibility, owned discovery, battery, pair/confirm/reject/PIN/passkey, sender validation, cancellation, connection/trust/block/remove, hotplug and BlueZ restart')
    finally:
        if driver and driver.poll() is None:driver.terminate();driver.wait(timeout=5)
        fixture.terminate();fixture.wait(timeout=5)
