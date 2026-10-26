#!/usr/bin/env python3
"""Native GPU-widget and SNI regression on an isolated bus/display."""
import asyncio,os,signal,sys,time,subprocess,tempfile,json
from dbus_next.aio import MessageBus
from pathlib import Path
from Xlib import display,X,XK
from Xlib.ext import xtest
from PIL import Image
ROOT=Path(__file__).resolve().parents[2]
BUILD=Path(os.environ.get('DOME_TRAY_BUILD_DIR',ROOT/'build/tray'))
signal.pthread_sigmask(signal.SIG_SETMASK, [])
assert os.environ.get('TRAY_TEST_PRIVATE_BUS')=='1'
processes=[]
with tempfile.TemporaryDirectory(prefix='dome-tray-ui-') as directory:
 runtime=Path(directory);runtime.chmod(0o700);out=ROOT/'artifacts/tray-frontends';out.mkdir(parents=True,exist_ok=True)
 def start(cmd,env,name,**kwargs):
  p=subprocess.Popen(cmd,env=env,stdout=(out/name).open('w'),stderr=subprocess.STDOUT,**kwargs);processes.append(p);return p
 def wait(fn):
  for _ in range(750):
   try:
    value=fn()
    if value:return value
   except Exception:pass
   time.sleep(.04)
  raise AssertionError('Timed out; see artifacts/tray-frontends')
 try:
  rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp'],os.environ.copy(),'ui-xvfb.log',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd) as pipe:disp=':'+pipe.readline().strip()
  env={**os.environ,'HOME':directory,'DISPLAY':disp,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_CURRENT_DESKTOP':'Dome','DE_XR_GL_TRACE':'1','FLTK_BACKEND':'x11','LIBGL_ALWAYS_SOFTWARE':'1','TRAY_FIXTURE_READY':str(runtime/'ready')}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
  env['DBUS_SYSTEM_BUS_ADDRESS']=env['DBUS_SESSION_BUS_ADDRESS']
  env['BLUETOOTH_FIXTURE_READY']=str(runtime/'bluetooth-ready')
  env['BLUETOOTH_FIXTURE_LOG']=str(runtime/'bluetooth-events.json')
  start([sys.executable,str(ROOT/'tests/tray/mock_bluetooth.py')],env,'ui-bluetooth.log');wait(lambda:(runtime/'bluetooth-ready').exists())
  env['NETWORK_FIXTURE_READY']=str(runtime/'network-ready');env['NETWORK_FIXTURE_LOG']=str(runtime/'network-events.json')
  start([sys.executable,str(ROOT/'tests/tray/mock_network.py')],env,'ui-network.log');wait(lambda:(runtime/'network-ready').exists())
  backup_source=runtime/'source 日本語';backup_target=runtime/'backup';backup_source.mkdir();backup_target.mkdir()
  (backup_source/'hello.txt').write_text('native backup UI fixture')
  profile_folder=runtime/'config/dome/backups';profile_folder.mkdir(parents=True)
  profile_id='c5d3ecae-6cb9-4d29-bd60-ec10a8c2793c'
  profile=profile_folder/(profile_id+'.json')
  profile.write_text(json.dumps({'id':profile_id,'name':'UI fixture','source':str(backup_source),'destination':str(backup_target),'schedule':'off'}));profile.chmod(0o600)
  env['TRAY_FIXTURE_CONFIG_REQUESTS']=str(runtime/'config-requests')
  start([sys.executable,str(ROOT/'tests/tray/mock_services.py')],env,'ui-services.log');wait(lambda:(runtime/'ready').exists())
  manager=start([str(BUILD/'dome-tray')],env,'ui-frontend.log')
  def dbus(path,method,*args):
   p=subprocess.run(['gdbus','call','--session','--dest','org.domed.Tray','--object-path',path,'--method',method,*args],env=env,capture_output=True,text=True,timeout=4)
   assert p.returncode==0,p.stderr
   return p.stdout
  wait(lambda:subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus','--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.NameHasOwner','org.domed.Tray'],capture_output=True,text=True).stdout.strip()=='(true,)')
  d=display.Display(disp)
  def window():return next((w for w in d.screen().root.query_tree().children if w.get_wm_class() and 'org.domed.Tray' in w.get_wm_class() and w.get_attributes().map_state==X.IsViewable),None)
  assert window() is None,'Default startup must stay in the tray'
  async def check_notifier():
   bus=await MessageBus().connect()
   try:
    node=await bus.introspect('org.domed.Tray','/TrayApplications')
    props=bus.get_proxy_object('org.domed.Tray','/TrayApplications',node).get_interface('org.freedesktop.DBus.Properties')
    values=await props.call_get_all('org.kde.StatusNotifierItem')
    assert values['ItemIsMenu'].value is True
    assert values['Id'].value=='dome.tray'
    assert values['Menu'].value=='/TrayApplications/Menu'
    images=values['IconPixmap'].value
    assert [(w,h) for w,h,_ in images]==[(24,24),(48,48)]
    for w,h,data in images:
     assert len(data)==w*h*4
     assert any(data[0::4]) and 0 in data[0::4],'Icon must retain transparency'
    for path,identity in [('/Network','dome.network'),('/Backups','dome.backup')]:
     node=await bus.introspect('org.domed.Tray',path)
     props=bus.get_proxy_object('org.domed.Tray',path,node).get_interface('org.freedesktop.DBus.Properties')
     native=await props.call_get_all('org.kde.StatusNotifierItem')
     assert native['Id'].value==identity
     assert native['IconPixmap'].value!=images,'Utility icons must be distinguishable from the manager'
    node=await bus.introspect('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher')
    props=bus.get_proxy_object('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher',node).get_interface('org.freedesktop.DBus.Properties')
    items=(await props.call_get('org.kde.StatusNotifierWatcher','RegisteredStatusNotifierItems')).value
    assert [i for i in items if i.startswith('org.domed.Tray')]==['org.domed.Tray/TrayApplications','org.domed.Tray/Network','org.domed.Tray/Backups'],items
   finally:bus.disconnect()
  asyncio.run(check_notifier())
  def key(name,ctrl=False):
   if ctrl:xtest.fake_input(d,X.KeyPress,d.keysym_to_keycode(XK.string_to_keysym('Control_L')))
   code=d.keysym_to_keycode(XK.string_to_keysym(name));xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code)
   if ctrl:xtest.fake_input(d,X.KeyRelease,d.keysym_to_keycode(XK.string_to_keysym('Control_L')))
   d.sync();time.sleep(.15)
  def click(w,x,y):
   pos=d.screen().root.translate_coords(w,0,0);xtest.fake_input(d,X.MotionNotify,x=pos.x+x,y=pos.y+y-28)
   xtest.fake_input(d,X.ButtonPress,1);xtest.fake_input(d,X.ButtonRelease,1);d.sync();time.sleep(.3)
  menu='/TrayApplications/Menu'
  layout=dbus(menu,'com.canonical.dbusmenu.GetLayout','0','1','[]')
  for label in ['Tray applications','KDE Connect','Input methods','Wallets','Translator','Screenshot','Bluetooth','Network','Backups']:assert label in layout,layout
  dbus(menu,'com.canonical.dbusmenu.Event','99','clicked','<0>','0')
  assert window() is None,'Unknown menu events must not open a page'
  for row,page in enumerate(['tray','devices','input','wallets','translator','screenshot','bluetooth','network','backup'],1):
   dbus(menu,'com.canonical.dbusmenu.Event',str(row),'clicked','<0>','0');w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.6)
   assert w.get_geometry().height==632, 'DomeD manager must reclaim the local menu bar space'
   geo=w.get_geometry();raw=w.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
   img=Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX')
   img.save(out/('ui-'+page+'.png'))
   # Native bitmap fonts deliberately use a small palette, without antialiasing.
   assert len(img.getcolors(geo.width*geo.height))>8,page
   if page=='input':
    assert (190,101,112) in [color for _,color in img.crop((28,140,438,310)).getcolors(410*170)],'Native input-method flags were not drawn'
   if page=='bluetooth':
    def events():return json.loads((runtime/'bluetooth-events.json').read_text())
    assert not any(e.get('key')=='Powered' for e in events()),'Opening Bluetooth must not power on a radio'
    click(w,600,102)
    wait(lambda:any(e.get('key')=='Powered' and e['value'] for e in events()))
    click(w,80,210);click(w,450,334)
    time.sleep(.6)
    geo=w.get_geometry();raw=w.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
    Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX').save(out/'ui-bluetooth-pairing.png')
    click(w,410,530)
    wait(lambda:any(e.get('op')=='paired' for e in events()))
    click(w,775,102)
    wait(lambda:any(e.get('op')=='StartDiscovery' for e in events()))
   if page=='devices':click(w,80,110)
   if page=='network':
    assert not (runtime/'network-events.json').exists(),'Opening Network must not change networking or scan'
    click(w,250,102)
    wait(lambda:any(e.get('key')=='WirelessEnabled' and not e['value'] for e in json.loads((runtime/'network-events.json').read_text())))
   if page=='backup':
    # Empty form errors stay inside the manager instead of escaping a widget callback.
    click(w,650,98);time.sleep(.2);assert manager.poll() is None
    click(w,465,98);key('Down');key('Return');time.sleep(.3)
    geo=w.get_geometry();raw=w.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
    Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX').save(out/'ui-backup-selected.png')
    click(w,220,408)
    wait(lambda:(backup_target/'hello.txt').exists())
    assert (backup_target/'hello.txt').read_text()=='native backup UI fixture'
   if page=='input':click(w,390,424)
   if page=='wallets':click(w,470,95);click(w,80,160)
   if page=='devices':
    from Xlib.protocol import event
    w.send_event(event.ClientMessage(window=w,client_type=d.intern_atom('WM_PROTOCOLS'),data=(32,[d.intern_atom('WM_DELETE_WINDOW'),X.CurrentTime,0,0,0])),event_mask=0);d.flush()
   elif page=='input':key('Escape')
   else:key('w',ctrl=True)
   wait(lambda:window() is None);assert manager.poll() is None
   if page=='bluetooth':wait(lambda:any(e.get('op')=='StopDiscovery' for e in events()))
   asyncio.run(check_notifier())
  async def check_input_menu():
   bus=await MessageBus().connect()
   try:
    node=await bus.introspect('org.domed.Tray','/InputMethods/Menu')
    menu=bus.get_proxy_object('org.domed.Tray','/InputMethods/Menu',node).get_interface('com.canonical.dbusmenu')
    async def rows(parent):return (await menu.call_get_layout(parent,1,[]))[1][2]
    root=[v.value for v in await rows(0)]
    assert [r[1]['label'].value for r in root[:3]]==['Input methods','Input method groups','Configuration']
    methods=[v.value for v in await rows(1)]
    english=next(r for r in methods if r[1]['label'].value=='English')
    japanese=next(r for r in methods if r[1]['label'].value=='Japanese')
    assert english[1]['icon-name'].value=='flag-us' and english[1]['toggle-state'].value==1
    assert japanese[1]['icon-name'].value=='flag-jp'
    from dbus_next import Variant
    await menu.call_event(japanese[0],'clicked',Variant('i',0),0)
    for _ in range(50):
     methods=[v.value for v in await rows(1)]
     if next(r for r in methods if r[0]==japanese[0])[1]['toggle-state'].value==1:break
     await asyncio.sleep(.04)
    else:raise AssertionError('Input selection did not update Fcitx')
    groups=[v.value for v in await rows(2)]
    writing=next(r for r in groups if r[1]['label'].value=='Writing')
    await menu.call_event(writing[0],'clicked',Variant('i',0),0)
    groups=[v.value for v in await rows(2)]
    assert next(r for r in groups if r[0]==writing[0])[1]['toggle-state'].value==1
    config=[v.value for v in await rows(3)]
    assert any(r[1]['label'].value=='Classic UI…' for r in config)
    assert any(r[1]['label'].value=='Current input method…' for r in config)
    await menu.call_event(7,'clicked',Variant('i',0),0)
   finally:bus.disconnect()
  asyncio.run(check_input_menu())
  wait(lambda:(runtime/'config-requests').exists() and 'fcitx://config/global' in (runtime/'config-requests').read_text())
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();key('w',ctrl=True)
  wait(lambda:window() is None)
  config=subprocess.run([str(BUILD/'fcitx5-configuration-fltk'),'fcitx://config/addon/classicui'],env=env,capture_output=True,timeout=4)
  assert config.returncode==0,config.stderr
  wait(lambda:'fcitx://config/addon/classicui' in (runtime/'config-requests').read_text())
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.5)
  key('w',ctrl=True);wait(lambda:window() is None)
  dbus(menu,'com.canonical.dbusmenu.Event','1','clicked','<0>','0');wait(window)
  w=wait(window)
  service_name=wait(lambda:window().get_full_property(d.intern_atom('_KDE_NET_WM_APPMENU_SERVICE_NAME'),X.AnyPropertyType)).value.decode()
  object_path=w.get_full_property(d.intern_atom('_KDE_NET_WM_APPMENU_OBJECT_PATH'),X.AnyPropertyType).value.decode()
  layout=subprocess.run(['gdbus','call','--session','--dest',service_name,'--object-path',object_path,
   '--method','com.canonical.dbusmenu.GetLayout','0','2','[]'],env=env,capture_output=True,text=True,timeout=4)
  assert layout.returncode==0,layout.stderr
  for label in ['_File','_Refresh','_Close','_View']:assert label in layout.stdout,layout.stdout
  assert '_Quit' not in layout.stdout
  second=subprocess.run([str(BUILD/'dome-tray'),'--devices'],env=env,capture_output=True,timeout=4);assert second.returncode==0
  assert len([w for w in d.screen().root.query_tree().children if w.get_wm_class() and 'org.domed.Tray' in w.get_wm_class()])==1
  translator=start([str(ROOT/'build/translator/dome-translator'),'--tray'],env,'ui-translator.log')
  def translator_owned():
   return 'true' in subprocess.run(['gdbus','call','--session','--dest','org.freedesktop.DBus',
    '--object-path','/org/freedesktop/DBus','--method','org.freedesktop.DBus.NameHasOwner',
    'org.domed.Translator'],env=env,capture_output=True,text=True).stdout
  wait(translator_owned)
  driver=subprocess.Popen([str(BUILD/'tray-service-driver')],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True);processes.append(driver)
  def service(op,**args):
   driver.stdin.write(json.dumps({'op':op,**args})+'\n');driver.stdin.flush()
   result=json.loads(driver.stdout.readline());assert result['ok'],result
   return result['value']
  items=service('tray');assert len([i for i in items if i['Id']=='dome.translator'])==1
  selected=next(i for i in items if i['Id']=='dome.translator')
  subprocess.run([str(BUILD/'dome-tray'),'--translator'],env=env,check=True,timeout=4)
  w=wait(window);time.sleep(.6)
  w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.2)
  click(w,250,524)
  geo=w.get_geometry();raw=w.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
  Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX').save(out/'ui-placement-out.png')
  wait(lambda:not next(i for i in service('tray') if i['Id']=='dome.translator')['managed'])
  time.sleep(.3);click(w,100,524)
  wait(lambda:next(i for i in service('tray') if i['Id']=='dome.translator')['managed'])
  print('PASS: native placement buttons move Translator out of and back into the manager')

  item=next(i for i in items if i['Id']=='dome.translator')
  rows=service('menu',item=item,parent=0);assert len(rows)==6 and rows[5]['checkable'] and not rows[5]['checked']
  service('event',item=item,id=4)
  wait(lambda:any(w.get_wm_name()=='DeepL API Free settings' and w.get_attributes().map_state==X.IsViewable for w in d.screen().root.query_tree().children))
  service('event',item=item,id=5);assert translator.wait(timeout=8)==0
  assert not service('menu',item=item,parent=0)[4]['enabled']
  print('PASS: translator has one manager entry, live toggle state, working settings and Quit, and returns to launchable state')
  log=(out/'ui-frontend.log').read_text();assert 'FLTK GPU widgets:' in log,log
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.3);key('q',ctrl=True);assert manager.poll() is None
  manager.terminate();assert manager.wait(timeout=4)==-15
  # Starting directly on a service page must also survive closing the window.
  manager=start([str(BUILD/'fcitx5-configuration-fltk'),'fcitx://config/global'],env,'ui-direct-frontend.log')
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();key('w',ctrl=True)
  wait(lambda:window() is None);assert manager.poll() is None
  dbus(menu,'com.canonical.dbusmenu.Event','1','clicked','<0>','0');wait(window)
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.3);key('q',ctrl=True);assert manager.poll() is None
  manager.terminate();assert manager.wait(timeout=4)==-15
  fallback=start([str(BUILD/'dome-tray'),'--tray'],{**env,'XDG_CURRENT_DESKTOP':'Other'},'ui-fallback.log')
  w=wait(window);assert w.get_geometry().height==660,'Other desktops must retain the local menu layout'
  w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.3);key('q',ctrl=True);assert fallback.poll() is None
  fallback.terminate();assert fallback.wait(timeout=4)==-15
  print('PASS: DomeD hides the local menu, exports its commands, preserves shortcuts, and other desktops retain the fallback')
  print('PASS: managed service icons, seven pages, network controls, real rsync backup, Bluetooth pairing/discovery, utility shortcuts, flags, Fcitx configuration, GPU presentation, close/restore and permanent service')
 finally:
  for p in reversed(processes):p.terminate()
  for p in reversed(processes):
   try:p.wait(timeout=4)
   except subprocess.TimeoutExpired:p.kill();p.wait()
