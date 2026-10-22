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
  for _ in range(150):
   try:
    value=fn()
    if value:return value
   except Exception:pass
   time.sleep(.04)
  raise AssertionError('Timed out; see artifacts/tray-frontends')
 try:
  rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x800x24','-nolisten','tcp'],os.environ.copy(),'ui-xvfb.log',pass_fds=(wr,));os.close(wr)
  with os.fdopen(rd) as pipe:disp=':'+pipe.readline().strip()
  env={**os.environ,'DISPLAY':disp,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),'XDG_STATE_HOME':str(runtime/'state'),'XDG_CURRENT_DESKTOP':'Dome','DE_XR_GL_TRACE':'1','FLTK_BACKEND':'x11','LIBGL_ALWAYS_SOFTWARE':'1','TRAY_FIXTURE_READY':str(runtime/'ready')}
  for key in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(key,None)
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
    node=await bus.introspect('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher')
    props=bus.get_proxy_object('org.kde.StatusNotifierWatcher','/StatusNotifierWatcher',node).get_interface('org.freedesktop.DBus.Properties')
    items=(await props.call_get('org.kde.StatusNotifierWatcher','RegisteredStatusNotifierItems')).value
    assert [i for i in items if i.startswith('org.domed.Tray')]==['org.domed.Tray/TrayApplications'],items
   finally:bus.disconnect()
  asyncio.run(check_notifier())
  def key(name,ctrl=False):
   if ctrl:xtest.fake_input(d,X.KeyPress,d.keysym_to_keycode(XK.string_to_keysym('Control_L')))
   code=d.keysym_to_keycode(XK.string_to_keysym(name));xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code)
   if ctrl:xtest.fake_input(d,X.KeyRelease,d.keysym_to_keycode(XK.string_to_keysym('Control_L')))
   d.sync();time.sleep(.15)
  def click(w,x,y):
   pos=w.translate_coords(d.screen().root,0,0);xtest.fake_input(d,X.MotionNotify,x=pos.x+x,y=pos.y+y)
   xtest.fake_input(d,X.ButtonPress,1);xtest.fake_input(d,X.ButtonRelease,1);d.sync();time.sleep(.3)
  menu='/TrayApplications/Menu'
  layout=dbus(menu,'com.canonical.dbusmenu.GetLayout','0','1','[]')
  for label in ['Tray applications','KDE Connect','Input methods','Wallets','Translator','Screenshot']:assert label in layout,layout
  dbus(menu,'com.canonical.dbusmenu.Event','99','clicked','<0>','0')
  assert window() is None,'Unknown menu events must not open a page'
  for row,page in enumerate(['tray','devices','input','wallets','translator','screenshot'],1):
   dbus(menu,'com.canonical.dbusmenu.Event',str(row),'clicked','<0>','0');w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.6)
   geo=w.get_geometry();raw=w.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
   img=Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX')
   img.save(out/('ui-'+page+'.png'))
   # Native bitmap fonts deliberately use a small palette, without antialiasing.
   assert len(img.getcolors(geo.width*geo.height))>8,page
   if page=='input':
    assert (190,101,112) in [color for _,color in img.crop((28,140,438,310)).getcolors(410*170)],'Native input-method flags were not drawn'
   if page=='devices':click(w,80,110)
   if page=='input':click(w,390,424)
   if page=='wallets':click(w,470,95);click(w,80,160)
   if page=='devices':
    from Xlib.protocol import event
    w.send_event(event.ClientMessage(window=w,client_type=d.intern_atom('WM_PROTOCOLS'),data=(32,[d.intern_atom('WM_DELETE_WINDOW'),X.CurrentTime,0,0,0])),event_mask=0);d.flush()
   elif page=='input':key('Escape')
   else:key('w',ctrl=True)
   wait(lambda:window() is None);assert manager.poll() is None
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
  item=next(i for i in items if i['Id']=='dome.translator')
  rows=service('menu',item=item,parent=0);assert len(rows)==6 and rows[5]['checkable'] and not rows[5]['checked']
  service('event',item=item,id=4)
  wait(lambda:any(w.get_wm_name()=='DeepL API Free settings' and w.get_attributes().map_state==X.IsViewable for w in d.screen().root.query_tree().children))
  service('event',item=item,id=5);assert translator.wait(timeout=8)==0
  assert not service('menu',item=item,parent=0)[4]['enabled']
  print('PASS: translator has one manager entry, live toggle state, working settings and Quit, and returns to launchable state')
  log=(out/'ui-frontend.log').read_text();assert 'FLTK GPU widgets:' in log,log
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();key('q',ctrl=True);assert manager.wait(timeout=4)==0
  # Starting directly on a service page must also survive closing the window.
  manager=start([str(BUILD/'fcitx5-configuration-fltk'),'fcitx://config/global'],env,'ui-direct-frontend.log')
  w=wait(window);w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();key('w',ctrl=True)
  wait(lambda:window() is None);assert manager.poll() is None
  dbus(menu,'com.canonical.dbusmenu.Event','1','clicked','<0>','0');wait(window)
  print('PASS: one tray entry, four pages, two utility shortcuts, flags, Fcitx method/group/configuration submenus, fcitx5-configuration-fltk URI routing, GPU presentation, close/restore and explicit Quit')
 finally:
  for p in reversed(processes):p.terminate()
  for p in reversed(processes):
   try:p.wait(timeout=4)
   except subprocess.TimeoutExpired:p.kill();p.wait()
