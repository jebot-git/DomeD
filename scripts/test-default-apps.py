#!/usr/bin/env python3
"""Default-handler backend and optional compositor UI checks in isolated XDG directories."""
import argparse,json,os,shutil,signal,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK,[])
root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--compositor',type=Path);args=parser.parse_args()
if args.compositor and os.environ.get('DOME_DEFAULT_APPS_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_DEFAULT_APPS_TEST_BUS':'1'})
out=root/'artifacts/default-apps';out.mkdir(parents=True,exist_ok=True)
flags=subprocess.check_output(['pkg-config','--cflags','--libs','Qt6Widgets','gio-unix-2.0'],text=True).split()
client=out/'default-apps-client'
subprocess.run(['c++','-std=c++17','-fPIC',str(root/'scripts/default-apps-client.cpp'),str(root/'native/default_apps.cpp'),'-o',str(client),*flags],check=True)
checks=[]
def check(value,label):
    checks.append({'passed':bool(value),'check':label});print(('PASS 'if value else'FAIL ')+label,flush=True);assert value,label
def wait(fn):
    deadline=time.monotonic()+15
    while time.monotonic()<deadline:
        try:
            result=fn()
            if result:return result
        except (OSError,ValueError,KeyError):pass
        time.sleep(.05)
    raise AssertionError('Timed out; see artifacts/default-apps')
with tempfile.TemporaryDirectory(prefix='dome-default-apps-')as directory:
    base=Path(directory);config=base/'config';apps=base/'data/applications';bin_dir=base/'bin';empty=base/'empty'
    for path in [config,apps,bin_dir,empty]:path.mkdir(parents=True)
    (apps.parent/'mime').symlink_to('/usr/share/mime',target_is_directory=True)
    env={**os.environ,'HOME':str(base),'XDG_CONFIG_HOME':str(config),'XDG_DATA_HOME':str(base/'data'),'XDG_DATA_DIRS':str(empty),'XDG_CONFIG_DIRS':str(empty),'XDG_CURRENT_DESKTOP':'Dome','PATH':str(bin_dir)+':'+os.environ['PATH']}
    journal=base/'opened'
    for executable,label in [('alpha','Alpha editor'),('beta','Beta editor'),('fsvr','fsvr'),('audio','Audio player')]:
        path=bin_dir/executable
        path.write_text('#!/bin/sh\nprintf "%s\\n" '+executable+' "$@" >> '+str(journal)+'\n');path.chmod(0o700)
        mime='inode/directory;x-scheme-handler/trash;'if executable=='fsvr'else'audio/mpeg;'if executable=='audio'else'text/plain;image/png;application/pdf;inode/directory;'
        desktop='org.fsvxr.fsvr.desktop'if executable=='fsvr'else executable+'.desktop'
        (apps/desktop).write_text(f'[Desktop Entry]\nType=Application\nName={label}\nExec={executable} %U\nTryExec={executable}\nMimeType={mime}\n')
    subprocess.run(['update-desktop-database',str(apps)],env=env,check=True)
    generic=config/'mimeapps.list';generic.write_text('[Default Applications]\ntext/plain=alpha.desktop;\nimage/png=alpha.desktop;\n')
    original=generic.read_bytes();scoped=config/'dome-mimeapps.list';scoped.write_text('# Keep this comment\n[Default Applications]\napplication/x-unrelated=keep.desktop;\n[Fixture Section]\nkeep=untouched\n')
    def listing():return json.loads(subprocess.check_output([str(client),'list'],env=env))
    def row(mime):return next(r for r in listing()['rows']if r['mime']==mime)
    def set_default(mime,app):return subprocess.run([str(client),'set',mime,app],env=env,capture_output=True,text=True)
    check(row('inode/directory')['current']['id']=='org.fsvxr.fsvr.desktop','Installed fsvr becomes the initial Dome folder handler')
    check(row('application/pdf')['current'] is None,'Absent Miro leaves Dome PDF choice unset even when other viewers are installed')
    miro_dir=bin_dir/'PDF viewers';miro_dir.mkdir()
    miro=miro_dir/'miro-pdf';miro.write_text('#!/bin/sh\nprintf "%s\\n" miro-pdf "$@" >> '+str(journal)+'\n');miro.chmod(0o700)
    miro_entry=apps/'miro-pdf.desktop'
    miro_entry.write_text('[Desktop Entry]\nType=Application\nName=Miro PDF\nExec="'+str(miro)+'" %f\nMimeType=application/pdf;\n')
    subprocess.run(['update-desktop-database',str(apps)],env=env,check=True)
    check(row('application/pdf')['current']['id']=='miro-pdf.desktop','Installed Miro becomes the initial Dome PDF viewer')
    check(generic.read_bytes()==original,'Miro recommendation preserves other desktops and global user preferences')
    pdf=base/'sample with spaces $literal.pdf';pdf.write_bytes(b'%PDF-1.4\n'+b' '*2048)
    subprocess.run(['gio','open',str(pdf)],env=env,check=True)
    wait(lambda:journal.exists()and'miro-pdf'in journal.read_text())
    check(str(pdf)in journal.read_text(),'gio open dispatches PDF paths literally to the Dome Miro default')
    check(set_default('application/pdf','alpha.desktop').returncode==0 and row('application/pdf')['current']['id']=='alpha.desktop','Explicit PDF user choice overrides the Miro recommendation')
    check(set_default('application/pdf','miro-pdf.desktop').returncode==0,'User can explicitly select installed Miro')
    miro.unlink();miro_entry.unlink();subprocess.run(['update-desktop-database',str(apps)],env=env,check=True)
    check(row('application/pdf')['current'] is None and 'application/pdf=miro-pdf.desktop;'in scoped.read_text(),'Removed Miro preserves the user choice without choosing another PDF viewer')
    check(set_default('application/pdf','beta.desktop').returncode==0 and row('application/pdf')['current']['id']=='beta.desktop','Without Miro the user can select another registered PDF handler')
    check(generic.read_bytes()==original,'Automatic file-manager preference preserves other desktops’ defaults')
    check({a['id']for a in row('text/plain')['apps']}=={'alpha.desktop','beta.desktop'},'Only registered handlers are offered for text files')
    check(not row('video/x-matroska')['apps'],'File types without installed handlers report an empty choice list')
    check(set_default('text/plain','beta.desktop').returncode==0,'Selecting a text handler saves successfully')
    check(row('text/plain')['current']['id']=='beta.desktop','Refresh reads the saved default immediately')
    check('# Keep this comment'in scoped.read_text()and'application/x-unrelated=keep.desktop;'in scoped.read_text()and'[Fixture Section]'in scoped.read_text(),'Saving preserves unrelated associations, sections and comments')
    check(generic.read_bytes()==original and row('image/png')['current']['id']=='alpha.desktop','A changed text handler preserves other file types and global defaults')
    path=base/'sample with spaces $literal.txt';path.write_text('hello')
    subprocess.run(['gio','open',path.as_uri()],env=env,check=True)
    wait(lambda:journal.exists()and'beta'in journal.read_text())
    check(str(path)in journal.read_text()or path.as_uri()in journal.read_text(),'gio open uses the selected handler and preserves literal file names')
    before=scoped.read_bytes()
    check(set_default('text/plain','audio.desktop').returncode!=0 and scoped.read_bytes()==before,'An incompatible handler is rejected without changing settings')
    check(set_default('text/plain','gone.desktop').returncode!=0 and scoped.read_bytes()==before,'A removed handler is rejected without changing settings')
    check(set_default('inode/directory','alpha.desktop').returncode==0 and row('inode/directory')['current']['id']=='alpha.desktop','An explicit folder choice takes precedence over automatic fsvr selection')
    good=scoped.read_bytes();scoped.write_text('not a valid key file\n');broken=scoped.read_bytes()
    check(set_default('text/plain','beta.desktop').returncode!=0 and scoped.read_bytes()==broken,'A malformed preferences file is reported and preserved')
    scoped.write_bytes(good)
    blocker=base/'not-a-directory';blocker.write_text('blocked')
    failed=subprocess.run([str(client),'set','text/plain','alpha.desktop'],env={**env,'XDG_CONFIG_HOME':str(blocker)},capture_output=True)
    check(failed.returncode!=0 and scoped.read_bytes()==good,'Write failures are reported without losing the saved associations')
    fsv_desktop=apps/'org.fsvxr.fsvr.desktop';fsv_metadata=fsv_desktop.read_text();fsv_desktop.unlink()
    fsx=bin_dir/'fsx-vr';fsx.write_text((bin_dir/'fsvr').read_text().replace('fsvr','fsx-vr'));fsx.chmod(0o700)
    (apps/'fsx-vr.desktop').write_text('[Desktop Entry]\nType=Application\nName=fsx-vr\nExec=fsx-vr %U\nTryExec=fsx-vr\nMimeType=inode/directory;x-scheme-handler/trash;\n')
    scoped.write_text('[Default Applications]\n')
    subprocess.run(['update-desktop-database',str(apps)],env=env,check=True)
    check(row('inode/directory')['current']['id']=='fsx-vr.desktop','Installed fsx-vr is preferred when fsvr is unavailable')
    fsv_desktop.write_text(fsv_metadata);scoped.write_bytes(good)
    subprocess.run(['update-desktop-database',str(apps)],env=env,check=True)
    if args.compositor:
        from Xlib import X,XK,display
        from Xlib.ext import xtest
        from PIL import Image
        processes=[];logs=[]
        def start(command,name,environment,**kwargs):
            log=(out/(name+'.log')).open('w');logs.append(log);p=subprocess.Popen(command,env=environment,stdout=log,stderr=log,**kwargs);processes.append(p);return p
        try:
            # Remove only these fixture choices to exercise automatic seeding in the compositor.
            scoped.unlink();journal.unlink(missing_ok=True)
            runtime=base/'runtime';runtime.mkdir(mode=0o700)
            rd,wr=os.pipe();start([os.environ.get('DOME_XVFB',shutil.which('Xvfb')or''),'-displayfd',str(wr),'-screen','0','1600x1000x24','-nolisten','tcp'],'xvfb',env,pass_fds=(wr,));os.close(wr)
            with os.fdopen(rd)as pipe:display_name=':'+pipe.readline().strip()
            with socket.socket()as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
            gui={**env,'DISPLAY':display_name,'XDG_RUNTIME_DIR':str(runtime),'DOME_PREVIEW_PORT':str(port),'DOME_RENDERER':'gles','DOME_START_XR':'0','DOME_START_IME':'0','DOME_LOGIN_SESSION':'0','LIBGL_ALWAYS_SOFTWARE':'1','RUST_LOG':'warn'}
            for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET','DOME_FILE_MANAGER']:gui.pop(name,None)
            start([str(args.compositor),'--backend','nested'],'compositor',gui)
            api=f'http://127.0.0.1:{port}'
            wait(lambda:json.load(urllib.request.urlopen(api+'/api/state',timeout=2)))
            xd=display.Display(display_name);host=wait(lambda:next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name()or'').startswith('Dome')),None));host.set_input_focus(X.RevertToParent,X.CurrentTime);xd.sync();time.sleep(.3)
            def click(x,y):
                xtest.fake_input(xd,X.MotionNotify,x=int(x),y=int(y));xd.sync()
                for pressed in [True,False]:xtest.fake_input(xd,X.ButtonPress if pressed else X.ButtonRelease,1);xd.sync();time.sleep(.08)
            def key(name,pressed=None):
                code=xd.keysym_to_keycode(XK.string_to_keysym(name));assert code,name
                for down in [True,False]if pressed is None else[pressed]:xtest.fake_input(xd,X.KeyPress if down else X.KeyRelease,code);xd.sync();time.sleep(.06)
            def shot(name):
                g=host.get_geometry();raw=host.get_image(0,0,g.width,g.height,X.ZPixmap,0xffffffff);Image.frombytes('RGB',(g.width,g.height),raw.data,'raw','BGRX').save(out/name)
            key('Super_L',True);key('e');key('Super_L',False)
            wait(lambda:journal.exists()and'fsvr'in journal.read_text());check(True,'Open Files launches the detected fsvr executable in the running compositor')
            click(20,18);click(100,222);click(500,620);time.sleep(.4);shot('default-applications.png')
            click(800,172);time.sleep(.2);shot('choose-text-handler.png')
            check('text/plain=beta.desktop;'not in scoped.read_text(),'Opening the chooser makes no change before confirmation')
            click(600,172);click(800,656)
            wait(lambda:'text/plain=beta.desktop;'in scoped.read_text())
            check(row('text/plain')['current']['id']=='beta.desktop','Appearance settings picker applies the confirmed text handler')
            key('Escape');click(20,18);click(100,558);time.sleep(.2);click(800,128);time.sleep(.2);click(600,128);click(800,656)
            wait(lambda:'inode/directory=alpha.desktop;'in scoped.read_text())
            key('Escape');journal.unlink(missing_ok=True)
            key('Super_L',True);key('e');key('Super_L',False)
            wait(lambda:journal.exists()and'alpha'in journal.read_text())
            check('fsvr'not in journal.read_text(),'Open Files honors an explicit alternative folder handler')
            xd.close()
        finally:
            for p in reversed(processes):
                if p.poll()is None:p.terminate()
            for p in reversed(processes):
                try:p.wait(timeout=5)
                except subprocess.TimeoutExpired:p.kill();p.wait()
            for log in logs:log.close()
(out/'report.json').write_text(json.dumps({'checks':checks,'failed':sum(not c['passed']for c in checks)},indent=2)+'\n')
