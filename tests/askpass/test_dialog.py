import os, subprocess, time, tempfile, json, shutil
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
from Xlib import display, X, XK
from Xlib.ext import xtest
exe=str(ROOT / 'build/askpass-stage/usr/local/bin/dome-askpass')
rd,wr=os.pipe()
xv=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','1024x768x24','-nolisten','tcp'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,pass_fds=(wr,))
os.close(wr)
with os.fdopen(rd) as pipe: display_name=':'+pipe.readline().strip()
fixture=tempfile.TemporaryDirectory(prefix='dome-askpass-appearance-')
runtime=Path(fixture.name);config=runtime/'config/dome';config.mkdir(parents=True)
appearance={'light':True,'accent':'#d47a12','font':'Galmuri11Bitmap / Regular','font_size':16}
(config/'appearance.json').write_text(json.dumps(appearance))
export=runtime/f'dome-appearance-{os.getpid()}';export.mkdir()
family=ROOT/'assets/fonts/galmuri-v2.40.4'
(export/'fonts.conf').write_text(f'<fontconfig><include>/etc/fonts/fonts.conf</include><dir>{family}</dir><cachedir>{runtime}/font-cache</cachedir><selectfont><acceptfont><pattern><patelt name="family"><string>Galmuri11Bitmap</string></patelt></pattern></acceptfont></selectfont></fontconfig>')
(runtime/'wayland-test.dome-session.json').write_text(json.dumps({'compositor_pid':os.getpid(),'realm':'desktop'}))
try:
 time.sleep(.5)
 d=display.Display(display_name)
 env={**os.environ,'DISPLAY':display_name,'FLTK_BACKEND':'x11','XDG_CURRENT_DESKTOP':'KDE','XDG_RUNTIME_DIR':str(runtime),'XDG_CONFIG_HOME':str(runtime/'config'),'DE_XR_APPEARANCE_TRACE':'1'}
 env.pop('WAYLAND_DISPLAY',None)
 env.pop('FONTCONFIG_FILE',None)
 def key(name,ctrl=False,shift=False):
  mods=[]
  for enabled,k in [(ctrl,'Control_L'),(shift,'Shift_L')]:
   if enabled:
    code=d.keysym_to_keycode(XK.string_to_keysym(k));mods.append(code);xtest.fake_input(d,X.KeyPress,code)
  code=d.keysym_to_keycode(XK.string_to_keysym(name));xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code)
  for code in reversed(mods):xtest.fake_input(d,X.KeyRelease,code)
  d.sync();time.sleep(.025)
 def run(action,expected):
  p=subprocess.Popen([exe,'Test authentication (disposable input)'],env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
  for _ in range(100):
   windows=[w for w in d.screen().root.query_tree().children if w.get_wm_class() and 'org.domed.Askpass' in w.get_wm_class()]
   if windows:break
   if p.poll() is not None:raise AssertionError(p.communicate()[1])
   time.sleep(.05)
  else:p.kill();raise AssertionError('No dialog')
  w=windows[0];w.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(.1)
  action()
  out,err=p.communicate(timeout=5)
  assert b'name=Galmuri11Bitmap / Regular size=16' in err,err
  assert b'color=3857313536' in err,err
  assert out==expected,(out,err)
  assert (p.returncode==0)==(expected is not None and bool(expected)),p.returncode
 for name,action,expected in [
  ('accept',lambda:[key(k) for k in ['a','b','space','5','Return']],b'ab 5\n'),
  ('empty',lambda:key('Return'),b'\n'),
  ('escape',lambda:key('Escape'),b''),
  ('ctrl-w',lambda:key('w',ctrl=True),b'')]:
  run(action,expected);print('PASS',name)
 env.pop('DISPLAY',None)
 p=subprocess.run([exe],env=env,capture_output=True);assert p.returncode and not p.stdout
 print('PASS no-display')
 p=subprocess.run([exe,'a','b'],env=env,capture_output=True);assert p.returncode and not p.stdout
 print('PASS invalid-arguments')
 p=subprocess.run([str(ROOT / 'build/askpass/fltk/pinentry-fltk')],input=b'GETINFO version\nBYE\n',capture_output=True)
 assert p.returncode==0 and b'D 1.3.3' in p.stdout
 print('PASS Assuan compatibility')
finally:
 xv.terminate();xv.wait();fixture.cleanup()
