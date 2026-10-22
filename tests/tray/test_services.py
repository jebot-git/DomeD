#!/usr/bin/env python3
"""Run with dbus-run-session -- python3 tests/tray/test_services.py."""
import json,os,signal,subprocess,tempfile,time,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
BUILD=Path(os.environ.get('DOME_TRAY_BUILD_DIR',ROOT/'build/tray'))
signal.pthread_sigmask(signal.SIG_SETMASK, [])
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1','Set TRAY_TEST_PRIVATE_BUS=1 only with dbus-run-session'
with tempfile.TemporaryDirectory(prefix='tray-services-') as directory:
 ready=Path(directory)/'ready'
 fixture=subprocess.Popen([sys.executable,str(ROOT/'tests/tray/mock_services.py')],env={**os.environ,'TRAY_FIXTURE_READY':str(ready)})
 try:
  for _ in range(100):
   if ready.exists():break
   assert fixture.poll() is None;time.sleep(.05)
  assert ready.exists()
  translator=Path(directory)/'dome-translator'
  translator.write_text('#!/bin/sh\nprintf \'%s\\n\' "$@" > "'+str(Path(directory)/'launched')+'"\n')
  translator.chmod(0o755)
  driver=subprocess.Popen([str(BUILD/'tray-service-driver')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,env={**os.environ,'PATH':directory+':'+os.environ['PATH'],'XDG_CONFIG_HOME':str(Path(directory)/'config')})
  def call(op,**args):
   driver.stdin.write(json.dumps({'op':op,**args})+'\n');driver.stdin.flush();out=json.loads(driver.stdout.readline());assert out['ok'],out;return out['value']
  assert call('devices')[0]['name']=='Fixture Phone'
  assert 'plugins' in call('device',id='test_phone')
  for action in ['pair','accept','cancel','unpair','ping','ring','url','text','plugin-on','plugin-off','refresh','rename']:call('device-action',id='test_phone',action=action,value='fixture')
  assert call('methods')['current']=='keyboard-us'
  call('input-action',action='select',value='anthy');assert call('methods')['current']=='anthy'
  call('save-group',group='Default',layout='jp',methods=[{'id':'anthy','layout':'jp'}]);assert call('group',group='Default')['methods'][0]['id']=='anthy'
  config=call('config',uri='fcitx://config/global');assert config['Behavior/Keep']=='preserved'
  call('save-config',uri='fcitx://config/global',values={'Behavior/ActiveByDefault':'True'})
  config=call('config',uri='fcitx://config/global');assert config=={'Behavior/ActiveByDefault':'True','Behavior/Keep':'preserved'}
  assert call('wallets')==['Fixture'];h=call('open',name='Fixture');assert h==7
  assert call('folders',handle=h)==['Passwords'];assert call('entries',handle=h,folder='Passwords')[0]['type']==1
  assert call('password',handle=h,folder='Passwords',key='test')=='disposable-secret'
  call('wallet-action',action='write',handle=h,folder='Passwords',key='new',value='test-only')
  assert call('password',handle=h,folder='Passwords',key='new')=='test-only'
  mapping={'username':'fixture','unicode':'Příliš 日本語 😀','empty':''}
  call('write-data',handle=h,folder='Passwords',key='map',data={'type':3,'data':json.dumps(mapping)})
  assert json.loads(call('read-data',handle=h,folder='Passwords',key='map')['data'])==mapping
  call('write-data',handle=h,folder='Passwords',key='binary',data={'type':2,'data':'00 01 ff 80'})
  assert call('read-data',handle=h,folder='Passwords',key='binary')['data']=='0001ff80'
  call('wallet-action',action='rename',handle=h,folder='Passwords',key='new',value='renamed')
  call('wallet-action',action='remove',handle=h,folder='Passwords',key='renamed')
  call('wallet-action',action='create-folder',handle=h,folder='FixtureFolder');assert 'FixtureFolder' in call('folders',handle=h)
  call('wallet-action',action='remove-folder',handle=h,folder='FixtureFolder');call('wallet-action',action='lock',handle=h)
  items=call('tray')
  translator=next(i for i in items if i['Id']=='dome.translator')
  screenshot=next(i for i in items if i['Id']=='dome.screenshot')
  assert len([i for i in items if i['Id']=='dome.translator'])==1
  menu=call('menu',item=translator,parent=0);assert len(menu)==6 and not menu[4]['enabled']
  call('event',item=translator,id=4)
  for _ in range(100):
   if (Path(directory)/'launched').exists():break
   time.sleep(.02)
  assert (Path(directory)/'launched').read_text().strip()=='--settings'
  assert call('menu',item=screenshot,parent=0)[0]['enabled']
  call('event',item=screenshot,id=1);assert Path(str(ready)+'.capture').exists()
  call('placement',item=translator,managed=False)
  assert not next(i for i in call('tray') if i['Id']=='dome.translator')['managed']
  call('placement',item=translator,managed=True)
  assert next(i for i in call('tray') if i['Id']=='dome.translator')['managed']
  assert (Path(directory)/'config/dome/tray-applications.json').stat().st_mode&0o777==0o600
  item=next(i for i in items if i['Id']=='fixture');assert item['Title']=='Fixture Tray';assert call('menu',item=item,parent=0)[0]['checked']
  call('event',item=item,id=10)
  driver.stdin.close();assert driver.wait(timeout=3)==0
  print('PASS: KDE Connect, Fcitx groups/configuration, wallet lifecycle and DBusMenu calls on isolated bus')
 finally:fixture.terminate();fixture.wait(timeout=3)
