#!/usr/bin/env python3
"""Exercise the actual libnm backend against a private NetworkManager protocol fixture."""
import json, os, selectors, subprocess, sys, tempfile, time
from pathlib import Path
from mock_network import ROOT, WIFI, WIRED, AP, UUID
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1'
ROOT_DIR=Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='dome-network-test-') as directory:
    env={**os.environ,'DBUS_SYSTEM_BUS_ADDRESS':os.environ['DBUS_SESSION_BUS_ADDRESS'],'NETWORK_FIXTURE_READY':directory+'/ready'}
    fixture=subprocess.Popen([sys.executable,str(ROOT_DIR/'tests/tray/mock_network.py')],env=env)
    driver=None
    try:
        for _ in range(100):
            if Path(env['NETWORK_FIXTURE_READY']).exists():break
            time.sleep(.03)
        else:raise AssertionError('Network fixture did not start')
        driver=subprocess.Popen([str(ROOT_DIR/'build/tray/network-service-driver')],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        poll=selectors.DefaultSelector();poll.register(driver.stdout,selectors.EVENT_READ)
        def call(op,okay=True,**args):
            driver.stdin.write(json.dumps({'op':op,**args})+'\n');driver.stdin.flush()
            assert poll.select(10),'Network backend timed out'
            result=json.loads(driver.stdout.readline());assert result['ok']==okay,result
            return result.get('value') if okay else result['error']
        def action(operation,okay=True,**args):return call('action',okay,request={'action':operation,**args})
        def control(member,*args):
            return subprocess.check_output(['gdbus','call','--session','--dest','org.domed.NetworkFixture','--object-path',ROOT,'--method','org.domed.NetworkFixture.'+member,*args],env=env,text=True)
        state=call('snapshot');assert len(state['devices'])==2 and state['enabled']
        assert state['networks'][0]['name']=='Česká síť' and state['networks'][0]['security']=='wpa-psk'
        assert state['devices'][0]['addresses']==['192.0.2.10/24'] and state['devices'][0]['dns']==['192.0.2.1']
        assert control('Events').strip()=="('[]',)",'Opening network status must not mutate networking'
        action('wireless',enabled=False);assert not call('snapshot')['wireless']
        action('wireless',enabled=True);action('enabled',enabled=False);assert not call('snapshot')['enabled']
        action('enabled',enabled=True);action('scan',device=WIFI)
        action('scan',False,device=WIRED)
        action('activate',uuid=UUID);assert call('snapshot')['profiles'][0]['active']
        action('deactivate',uuid=UUID);assert not call('snapshot')['profiles'][0]['active']
        p=call('profile',uuid=UUID);assert p['name']=='Saved Wi-Fi'
        action('edit',uuid=UUID,name='Renamed 日本語',autoconnect=False)
        p=call('profile',uuid=UUID);assert p['name']=='Renamed 日本語' and not p['autoconnect']
        action('wifi',device=WIFI,path=AP,password='',ssid='ignored',name='ignored') # saved profile
        action('disconnect',device=WIFI)
        action('wifi',device=WIFI,path=AP,ssid='Česká síť'.encode().hex(),name='New Wi-Fi',password='fixture-password',security='wpa-psk')
        assert len(call('snapshot')['profiles'])==2
        action('wired',device=WIRED);assert len(call('snapshot')['profiles'])==3
        control('Deny','true')
        assert 'denied' in action('wireless',False,enabled=False)
        assert 'denied' in action('activate',False,uuid=UUID)
        control('Deny','false')
        action('delete',uuid=UUID);assert len(call('snapshot')['profiles'])==2
        action('activate',False,uuid=UUID)
        control('RemoveDevice');assert len(call('snapshot')['devices'])==1
        action('scan',False,device=WIFI)
        control('Disappear');assert 'not running' in call('snapshot',False)
        control('Reappear');assert len(call('snapshot')['devices'])==1
        print('PASS: libnm status, Unicode SSIDs, IP/DNS, explicit radio/scan actions, saved/new Wi-Fi, wired, activation, profile editing/deletion, policy errors, device removal and service recovery')
    finally:
        if driver:
            driver.stdin.close()
            try:driver.wait(timeout=3)
            except subprocess.TimeoutExpired:driver.kill();driver.wait()
        fixture.terminate();fixture.wait(timeout=3)
