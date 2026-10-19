#!/usr/bin/env python3
"""Clipboard and real KWallet/Secret Service on a private D-Bus, HOME and Xvfb.
Never opens the user's wallet or clipboard. Test credentials are disposable.
Run with uv --with dbus-next --with python-xlib --with pillow.
"""
import asyncio
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from dbus_next.aio import MessageBus
from dbus_next import Variant
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image

if os.environ.get('DOME_SERVICES_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_SERVICES_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1]
out=root/'artifacts/session-services';out.mkdir(parents=True,exist_ok=True)
processes=[];logs=[];checks=[];wallet_pids=set();capabilities={}
def check(ok,label):
    checks.append({'check':label,'passed':bool(ok)});print(('PASS ' if ok else 'FAIL ')+label,flush=True)
    assert ok,label

def start(args,name,env,**kwargs):
    log=(out/(name+'.log')).open('w');logs.append(log)
    child=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT,**kwargs);processes.append(child);return child

def wait(fn):
    deadline=time.monotonic()+20
    while time.monotonic()<deadline:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError):pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/session-services')

async def main(runtime):
    with socket.socket() as port_sock:port_sock.bind(('127.0.0.1',0));port=port_sock.getsockname()[1]
    base=f'http://127.0.0.1:{port}'
    def scene():return json.load(urllib.request.urlopen(base+'/api/state',timeout=2))['scene']
    async def eventually(fn):
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            result=await fn()
            if result:return result
            await asyncio.sleep(.05)
        raise AssertionError('Timed out waiting for D-Bus service')
    rd,wr=os.pipe()
    xvfb=str(root/'.local-deps/root/usr/bin/Xvfb')
    start([xvfb,'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
    with os.fdopen(rd) as pipe:display_name=':'+pipe.readline().strip()
    # Build a minimal child environment so the fake PAM socket cannot receive host credentials.
    env={'PATH':os.environ.get('PATH','/usr/bin:/bin'),'DBUS_SESSION_BUS_ADDRESS':os.environ['DBUS_SESSION_BUS_ADDRESS'],
        'HOME':str(runtime/'home'),'XDG_RUNTIME_DIR':str(runtime),'XDG_CONFIG_HOME':str(runtime/'config'),
        'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),'DISPLAY':display_name,
        'XDG_CURRENT_DESKTOP':'Dome','XDG_SESSION_DESKTOP':'dome','XDG_SESSION_TYPE':'wayland','QT_QPA_PLATFORM':'wayland',
        'LIBGL_ALWAYS_SOFTWARE':'1','DOME_PREVIEW_PORT':str(port),'DOME_LOGIN_SESSION':'1','DOME_START_XR':'0','RUST_LOG':'warn',
        'PAM_KWALLET5_LOGIN':str(runtime/'pam.sock')}
    for name in ['home','config','data','state']:(runtime/name).mkdir()
    (runtime/'config/kwalletrc').write_text('[Wallet]\nEnabled=true\nFirst Use=false\nClose When Idle=false\n')
    portal_dir=runtime/'config/xdg-desktop-portal';portal_dir.mkdir()
    (portal_dir/'dome-portals.conf').write_text((root/'packaging/dome-portals.conf').read_text())
    bus=await MessageBus().connect()
    dbus=bus.get_proxy_object('org.freedesktop.DBus','/org/freedesktop/DBus',await bus.introspect('org.freedesktop.DBus','/org/freedesktop/DBus')).get_interface('org.freedesktop.DBus')
    await dbus.call_update_activation_environment(env)
    received={}
    listener=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);listener.bind(env['PAM_KWALLET5_LOGIN']);listener.listen(1)
    def pam_receiver():
        conn,_=listener.accept()
        with conn:
            data=b''
            while chunk:=conn.recv(4096):data+=chunk
        received.update(line.split('=',1) for line in data.decode().splitlines() if '=' in line)
    threading.Thread(target=pam_receiver,daemon=True).start()
    start([os.environ.get('DOME_COMPOSITOR',str(root/'target/release/dome-compositor')),'--backend','nested'],'compositor',env)
    wait(lambda:scene()['source']=='wayland')
    log=(out/'compositor.log').read_text();wayland=re.search(r'Dome Wayland socket: (\S+)',log)[1]
    wait(lambda:received.get('WAYLAND_DISPLAY')==wayland)
    check(received.get('QT_QPA_PLATFORM')=='wayland','PAM handoff receives the ready DomeD Wayland display')
    listener.close()
    client_env={**env,'WAYLAND_DISPLAY':wayland}
    wlcopy=str(root/'.local-deps/root/usr/bin/wl-copy');wlpaste=str(root/'.local-deps/root/usr/bin/wl-paste')
    def copy(value,args=()):
        child=start([wlcopy,'--foreground',*args],'clipboard-'+str(len(processes)),client_env,stdin=subprocess.PIPE)
        child.stdin.write(value);child.stdin.close()
    def paste(args=()):
        result=subprocess.run([wlpaste,'--no-newline',*args],env=client_env,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL,timeout=3)
        return result.stdout if result.returncode==0 else None
    copy('DomeD clipboard — Unicode'.encode())
    wait(lambda:paste()=='DomeD clipboard — Unicode'.encode())
    check(True,'wl-copy/wl-paste exchange Unicode text without keyboard focus')
    copy(b'DomeD primary selection',['--primary'])
    wait(lambda:paste(['--primary'])==b'DomeD primary selection')
    check(paste()=='DomeD clipboard — Unicode'.encode(),'Primary selection is independent of the regular clipboard')
    payload=bytes(range(256))*4
    copy(payload,['--type','application/octet-stream'])
    wait(lambda:paste(['--type','application/octet-stream'])==payload)
    check(True,'Clipboard transfers arbitrary MIME data without corrupting bytes')
    # The same unfocused clipboard API used by KDE Connect exercises ext-data-control.
    kde_clipboard=start([str(root/'build/kde-clipboard-client')],'kde-clipboard',client_env)
    wait(lambda:paste()==b'DomeD KDE background clipboard')
    check(True,'KDE Connect clipboard API can write without a focused window')
    copy(b'DomeD external clipboard update')
    wait(lambda:'DomeD external clipboard update' in (out/'kde-clipboard.log').read_text())
    check(True,'KDE Connect clipboard API receives background clipboard updates')
    kde_clipboard.terminate();kde_clipboard.wait(timeout=3)
    await eventually(lambda:dbus.call_name_has_owner('org.freedesktop.Notifications'))
    notices=bus.get_proxy_object('org.freedesktop.Notifications','/org/freedesktop/Notifications',await bus.introspect('org.freedesktop.Notifications','/org/freedesktop/Notifications')).get_interface('org.freedesktop.Notifications')
    check(await notices.call_get_capabilities()==['body','actions'],'Notification server advertises its implemented capabilities')
    check((await notices.call_get_server_information())[0]=='DomeD','DomeD owns the private notification service')
    closed=[];invoked=[]
    notices.on_notification_closed(lambda id,reason:closed.append((id,reason)))
    notices.on_action_invoked(lambda id,key:invoked.append((id,key)))
    async def has_closed(id,reason):return (id,reason) in closed
    async def has_action(id,key):return (id,key) in invoked
    nd=display.Display(display_name)
    nhost=wait(lambda:next((w for w in nd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')),None))
    nhost.set_input_focus(X.RevertToParent,X.CurrentTime);nd.sync()
    def shot():
        g=nhost.get_geometry();raw=nhost.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff)
        return Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX')
    def nclick(x,y):
        g=nhost.get_geometry();xtest.fake_input(nd,X.MotionNotify,x=int(x+g.x),y=int(y+g.y));nd.sync();time.sleep(.08)
        for press in [True,False]:xtest.fake_input(nd,X.ButtonPress if press else X.ButtonRelease,1);nd.sync();time.sleep(.05)
    before=shot();focused=scene()['focused'];width=before.width
    id=await notices.call_notify('KDE Connect',0,'','Incoming phone notification','DomeD action and Unicode test — ✓',['accept','Accept','reject','Reject'],{},0)
    check(id>0,'Notify returns a nonzero ID')
    await asyncio.sleep(.3)
    shown=shot();shown.save(out/'notifications-actions.png')
    check(before.getpixel((width-350,60))!=shown.getpixel((width-350,60)),'Notification toast is visible in the desktop framebuffer')
    check(scene()['focused']==focused,'Incoming notification preserves keyboard focus')
    replaced=await notices.call_notify('KDE Connect',id,'','Updated pairing request','Replacement stays in the same toast',['accept','Accept','reject','Reject'],{},0)
    check(replaced==id and not closed,'Replacing a notification reuses its ID without closing it')
    await asyncio.sleep(.2);nclick(width-290,160)
    await eventually(lambda:has_action(id,'accept'));await eventually(lambda:has_closed(id,2))
    check(True,'Clicking a toast action emits ActionInvoked and user-dismissed NotificationClosed')
    expiring=await notices.call_notify('DomeD test',0,'','Expires','Short-lived toast',[],{},120)
    await eventually(lambda:has_closed(expiring,1));check(True,'Timeout emits NotificationClosed with expired reason')
    persistent=await notices.call_notify('DomeD test',0,'','Persistent','Never expires',[],{},0)
    await asyncio.sleep(.3);check(not any(i==persistent for i,_ in closed),'Zero timeout keeps a notification active')
    await notices.call_close_notification(persistent)
    await eventually(lambda:has_closed(persistent,3));check(True,'CloseNotification emits the explicit-close reason')
    resident=await notices.call_notify('KDE Connect',0,'','Default action','Click anywhere',['default','Open'],{'resident':Variant('b',True)},0)
    await asyncio.sleep(.15);nclick(width-200,88)
    await eventually(lambda:has_action(resident,'default'))
    check(not any(i==resident for i,_ in closed),'Resident notification remains after its default action')
    nclick(width-26,62);await eventually(lambda:has_closed(resident,2));check(True,'Close gadget dismisses a resident notification')
    critical=await notices.call_notify('DomeD test',0,'','Critical','Urgent requests remain visible',[],{'urgency':Variant('y',2)},100)
    await asyncio.sleep(.3);check(not any(i==critical for i,_ in closed),'Critical notifications remain until acknowledged')
    await notices.call_close_notification(critical)
    xr_notice=await notices.call_notify('KDE Connect',0,'','Headset request','XR action test',['accept','Accept'],{},0)
    await asyncio.sleep(.2)
    xr_layout=json.load(urllib.request.urlopen(base+'/api/xr-shell',timeout=2))
    check(xr_layout.get('notifications')==[652,48,360,132],'XR notification exports its own crop instead of stretching the upper bar')
    with urllib.request.urlopen(base+'/api/xr-frame/18446744073709551612',timeout=2)as response:
        texture=Image.frombytes('RGBA',(int(response.headers['X-Dome-Width']),int(response.headers['X-Dome-Height'])),response.read())
        texture.save(out/'notification-xr.png')
        check(texture.size==(360,132),'Headset notification texture retains the toast aspect ratio')
    packet={'events':[{'kind':'motion','id':18446744073709551612,'uv':[.25,112/132]},{'kind':'button','button':0x110,'pressed':True},{'kind':'button','button':0x110,'pressed':False}],'actions':[]}
    with urllib.request.urlopen(urllib.request.Request(base+'/api/xr-input',data=json.dumps(packet).encode(),headers={'Content-Type':'application/json'}),timeout=2)as response:assert response.status==200
    await eventually(lambda:has_action(xr_notice,'accept'));await eventually(lambda:has_closed(xr_notice,2))
    check(True,'XR pointer invokes the notification action and dismisses its shared desktop toast')
    nd.close()
    await eventually(lambda:dbus.call_name_has_owner('org.freedesktop.secrets'))
    wallet_pids.add(await dbus.call_get_connection_unix_process_id('org.freedesktop.secrets'))
    check(True,'DomeD starts the installed KWallet Secret Service on its session bus')
    secret=bus.get_proxy_object('org.freedesktop.secrets','/org/freedesktop/secrets',await bus.introspect('org.freedesktop.secrets','/org/freedesktop/secrets')).get_interface('org.freedesktop.Secret.Service')
    _,session=await secret.call_open_session('plain',Variant('s',''))
    collection,prompt=await secret.call_create_collection({'org.freedesktop.Secret.Collection.Label':Variant('s','dome-test-wallet')},'dome-test')
    completed=[]
    if prompt!='/':
        prompt_obj=bus.get_proxy_object('org.freedesktop.secrets',prompt,await bus.introspect('org.freedesktop.secrets',prompt)).get_interface('org.freedesktop.Secret.Prompt')
        prompt_obj.on_completed(lambda dismissed,result:completed.append((dismissed,result)))
        await prompt_obj.call_prompt('')
        xd=display.Display(display_name)
        host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or '').startswith('Dome')),None))
        host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
        await asyncio.sleep(.5)
        wait(lambda:len(scene()['windows'])>0)
        def key(name,down=None):
            code=xd.keysym_to_keycode(XK.string_to_keysym(name));assert code,name
            for press in ([True,False] if down is None else [down]):xtest.fake_input(xd,X.KeyPress if press else X.KeyRelease,code);xd.sync();time.sleep(.03)
        def screenshot(name):
            geo=host.get_geometry();raw=host.get_image(0,0,geo.width,geo.height,X.ZPixmap,0xffffffff)
            Image.frombytes('RGB',(geo.width,geo.height),raw.data,'raw','BGRX').save(out/(name+'.png'))
        screenshot('wallet-encryption-dialog')
        def click(x,y):
            geo=host.get_geometry();xtest.fake_input(xd,X.MotionNotify,x=int(x+geo.x),y=int(y+geo.y));xd.sync()
            for press in [True,False]:xtest.fake_input(xd,X.ButtonPress if press else X.ButtonRelease,1);xd.sync();time.sleep(.05)
        # The isolated wallet has no GPG keys: select password-encrypted storage.
        r=scene()['windows'][0]['desktop'];click(r['x']+242,r['y']+124);click(r['x']+583,r['y']+436)
        await asyncio.sleep(.5)
        screenshot('wallet-password-dialog')
        current=scene()
        host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync()
        r=current['windows'][-1]['desktop'];click(r['x']+200,r['y']+140)
        for char in 'r7fncx6mt9kqv3sp58bw12':key(char)
        key('Tab')
        for char in 'r7fncx6mt9kqv3sp58bw12':key(char)
        key('Return')
        await asyncio.sleep(.5)
        screenshot('wallet-after-password')
        async def collection_ready():return completed
        await eventually(collection_ready)
        check(not completed[0][0],'KWallet password dialog accepts input inside DomeD')
        collection=completed[0][1].value
        xd.close()
    collection_iface=bus.get_proxy_object('org.freedesktop.secrets',collection,await bus.introspect('org.freedesktop.secrets',collection)).get_interface('org.freedesktop.Secret.Collection')
    value=b'DomeD disposable wallet round-trip'
    item,item_prompt=await collection_iface.call_create_item({'org.freedesktop.Secret.Item.Label':Variant('s','DomeD test entry'),'org.freedesktop.Secret.Item.Attributes':Variant('a{ss}',{'application':'dome-test'})},[session,b'',value,'text/plain'],True)
    assert item_prompt=='/'
    item_iface=bus.get_proxy_object('org.freedesktop.secrets',item,await bus.introspect('org.freedesktop.secrets',item)).get_interface('org.freedesktop.Secret.Item')
    actual=await item_iface.call_get_secret(session)
    check(actual[2]==value,'KWallet Secret Service stores and retrieves a disposable test secret')
    await dbus.call_start_service_by_name('org.kde.kwalletd6',0)
    wallet_pids.add(await dbus.call_get_connection_unix_process_id('org.kde.kwalletd6'))
    wallet=bus.get_proxy_object('org.kde.kwalletd6','/modules/kwalletd6',await bus.introspect('org.kde.kwalletd6','/modules/kwalletd6')).get_interface('org.kde.KWallet')
    check('dome-test-wallet' in await wallet.call_wallets(),'KWallet compatibility API sees the test collection')
    # The KDE Connect daemon gets a private network namespace: no packets or pairing
    # requests can reach the user's devices, even if it enables discovery by default.
    kde_daemon=start(['unshare','--user','--map-current-user','--net','/usr/bin/kdeconnectd'],'kdeconnect-daemon',client_env)
    await eventually(lambda:dbus.call_name_has_owner('org.kde.kdeconnect'))
    cli=subprocess.run(['/usr/bin/kdeconnect-cli','--list-devices','--id-only'],env=client_env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=10)
    check(cli.returncode==0,'KDE Connect daemon and CLI work on the DomeD session bus')
    capabilities['kdeconnect_daemon']=True
    start(['/usr/bin/kdeconnect-app'],'kdeconnect-app',client_env)
    wait(lambda:any('connect' in w['app_id'].lower() for w in scene()['windows']))
    check(True,'KDE Connect graphical interface opens as a native DomeD Wayland window')
    capabilities['kdeconnect_gui']=True
    from dbus_next import Message
    import xml.etree.ElementTree as ET
    portal_reply=await bus.call(Message(destination='org.freedesktop.portal.Desktop',path='/org/freedesktop/portal/desktop',interface='org.freedesktop.DBus.Introspectable',member='Introspect'))
    portal_xml=ET.fromstring(portal_reply.body[0])
    capabilities['remote_input']=any(i.get('name')=='org.freedesktop.portal.RemoteDesktop' for i in portal_xml.findall('interface'))
    capabilities['notification_server']=await dbus.call_name_has_owner('org.freedesktop.Notifications')
    capabilities['phone_pairing_and_file_transfer']='not tested; no device pairing or network traffic'
    print('KDE Connect capability audit:',json.dumps(capabilities),flush=True)
    await item_iface.call_delete()
    await collection_iface.call_delete()
    check(True,'Disposable wallet data can be deleted cleanly')
    bus.disconnect()

result=1
try:
    with tempfile.TemporaryDirectory(prefix='dome-services-') as directory:
        runtime=Path(directory);runtime.chmod(0o700)
        try:
            asyncio.run(main(runtime))
            result=0
        finally:
            import signal
            for pid in wallet_pids:
                try:os.kill(pid,signal.SIGTERM)
                except ProcessLookupError:pass
            doc=runtime/'doc'
            if doc.is_mount():subprocess.run(['fusermount3','-u',str(doc)],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)

except Exception as error:
    import traceback;traceback.print_exc()
    print('SERVICES TEST FAILED:',repr(error),flush=True)
    checks.append({'check':str(error),'passed':False})
finally:
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
            try:child.wait(timeout=4)
            except subprocess.TimeoutExpired:child.kill();child.wait()
    for log in logs:log.close()
    (out/'report.json').write_text(json.dumps({'checks':checks,'capabilities':capabilities,'failed':sum(not c['passed'] for c in checks)},indent=2))
sys.exit(result)
