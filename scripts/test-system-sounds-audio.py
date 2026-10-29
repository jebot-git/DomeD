#!/usr/bin/env python3
"""Decode a real libcanberra event into a private virtual audio sink, never hardware."""
import argparse,array,asyncio,hashlib,json,os,re,socket,subprocess,sys,tempfile,time,urllib.request
from pathlib import Path
from dbus_next.aio import MessageBus
if os.environ.get('DOME_SOUND_AUDIO_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],{**os.environ,'DOME_SOUND_AUDIO_BUS':'1'})
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
R=Path(__file__).resolve().parents[1];out=a.output.resolve();out.mkdir(exist_ok=False);children=[];logs=[];checks=[];failure=None;env=None

def start(args,name,**kw):
    log=(out/(name+'.log')).open('wb');logs.append(log);proc=subprocess.Popen(list(map(str,args)),env=env,stderr=log,stdout=kw.pop('stdout',log),**kw);children.append(proc);return proc

def command(*args):return subprocess.check_output(args,env=env,stderr=subprocess.DEVNULL,timeout=5)

def wait(fn,seconds=20):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        try:
            value=fn()
            if value:return value
        except (OSError,ValueError,KeyError,subprocess.SubprocessError):pass
        time.sleep(.04)
    raise AssertionError('Private event audio timed out')

def check(ok,label):
    checks.append({'name':label,'passed':bool(ok)});print(('PASS 'if ok else'FAIL ')+label,flush=True);assert ok,label

def stop(proc):
    if proc.poll()is None:
        proc.terminate()
        try:proc.wait(timeout=8)
        except subprocess.TimeoutExpired:proc.kill();proc.wait()

async def notify():
    bus=await MessageBus().connect()
    try:
        obj=await bus.introspect('org.freedesktop.Notifications','/org/freedesktop/Notifications')
        interface=bus.get_proxy_object('org.freedesktop.Notifications','/org/freedesktop/Notifications',obj).get_interface('org.freedesktop.Notifications')
        return await interface.call_notify('Audio fixture',0,'','Private event','Virtual sink only',[],{},5000)
    finally:bus.disconnect()

try:
    with tempfile.TemporaryDirectory(prefix='dome-system-sounds-audio-')as folder:
        runtime=Path(folder);runtime.chmod(0o700)
        with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        env={k:v for k,v in os.environ.items()if not k.startswith('DOME_')}
        env.update(HOME=folder,XDG_RUNTIME_DIR=folder,PIPEWIRE_RUNTIME_DIR=folder,PIPEWIRE_REMOTE='pipewire-0',PULSE_SERVER='unix:'+folder+'/pulse/native',XDG_CONFIG_HOME=folder+'/config',XDG_STATE_HOME=folder+'/state',XDG_DATA_HOME=folder+'/data',XDG_CACHE_HOME=folder+'/cache',XDG_DATA_DIRS='/usr/local/share:/usr/share',DBUS_SYSTEM_BUS_ADDRESS='unix:path='+folder+'/no-system-bus',DOME_LOGIN_SESSION='0',DOME_SESSION_RESTORE='0',DOME_START_XR='0',DOME_RENDERER='gles',DOME_APPEARANCE_SERVICE='0',DOME_PREVIEW_PORT=str(port),LIBGL_ALWAYS_SOFTWARE='1',RUST_LOG='warn')
        for name in ['TRAY','KDECONNECT','KWALLET','POLKIT','IME','XWAYLAND']:env['DOME_START_'+name]='0'
        for name in ['WAYLAND_DISPLAY','WAYLAND_SOCKET']:env.pop(name,None)
        start(['pipewire','-c',R/'scripts/audio-test.conf'],'pipewire');wait(lambda:(runtime/'pipewire-0').is_socket())
        start(['wireplumber','--profile','policy'],'policy')
        def nodes():return {o['info']['props']['node.name']:o for o in json.loads(command('pw-dump'))if o.get('type')=='PipeWire:Interface:Node'}
        wait(lambda:len(nodes())==4)
        check(all(name.startswith('dome.test.')for name in nodes()),'Private graph starts with virtual devices only')
        sink_id=str(nodes()['dome.test.speakers']['id'])
        command('wpctl','set-default',sink_id)
        command('wpctl','set-volume',sink_id,'0.75')
        start(['pipewire-pulse','-c','/usr/share/pipewire/pipewire-pulse.conf'],'pulse');wait(lambda:(runtime/'pulse/native').is_socket())
        wait(lambda:re.search(r'^Default Sink: dome\.test\.speakers$',command('pactl','info').decode(),re.M))
        (out/'pulse-info.txt').write_bytes(command('pactl','info'))
        check('dome.test.speakers.monitor'in command('pactl','list','short','sources').decode(),'Private PulseAudio server exposes the virtual sink monitor')
        rd,wr=os.pipe();start(['Xvfb','-displayfd',str(wr),'-screen','0','1280x900x24','-nolisten','tcp'],'xvfb',pass_fds=(wr,));os.close(wr);env['DISPLAY']=':'+os.read(rd,64).decode().strip();os.close(rd)
        compositor=start([a.binary.resolve(),'--backend','nested'],'compositor')
        def state():
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/api/state',timeout=2)as response:return json.load(response)
        wait(lambda:state()['scene']['source']=='wayland');time.sleep(.4)
        pcm=out/'event.pcm';record=pcm.open('wb');logs.append(record)
        recorder=start(['parec','--device=dome.test.speakers.monitor','--format=s16le','--rate=48000','--channels=2'],'monitor',stdout=record)
        wait(lambda:pcm.stat().st_size>=19200)
        baseline=pcm.stat().st_size
        subscription=start(['pactl','subscribe'],'pulse-events')
        check(asyncio.run(notify())>0,'Native server accepts the audible notification')
        end=time.monotonic()+5
        while time.monotonic()<end:
            data=pcm.read_bytes();probe=array.array('h');probe.frombytes(data[:len(data)//2*2])
            if max(map(abs,probe),default=0)>256:break
            time.sleep(.05)
        stop(recorder);record.flush()
        values=array.array('h');values.frombytes(pcm.read_bytes());peak=max(map(abs,values),default=0)
        before=array.array('h');before.frombytes(pcm.read_bytes()[:baseline]);quiet=max(map(abs,before),default=0)
        check(quiet==0,'Virtual output is silent before the notification')
        (out/'audio-measurement.json').write_text(json.dumps({'samples':len(values),'peak_s16':peak,'baseline_peak_s16':quiet},indent=2)+'\n')
        (out/'graph-after.json').write_bytes(command('pw-dump'))
        check(peak>256,'Real libcanberra decodes and plays audible PCM into the virtual sink')
        graph=nodes();sinks=[name for name,o in graph.items()if o['info']['props'].get('media.class')=='Audio/Sink']
        check(all(name.startswith('dome.test.')or name=='auto_null'for name in sinks),'Every output remains virtual; host audio graph is unused')
        check(compositor.poll()is None,'Compositor stays responsive after real event audio playback')
        (out/'audio-measurement.json').write_text(json.dumps({'samples':len(values),'peak_s16':peak,'baseline_peak_s16':quiet,'sink_names':sinks},indent=2)+'\n')
except BaseException as ex:
    failure=repr(ex);raise
finally:
    for proc in reversed(children):stop(proc)
    for log in logs:log.close()
    (out/'report.json').write_text(json.dumps({'passed':failure is None and bool(checks),'failure':failure,'checks':checks,'binary_sha256':hashlib.sha256(a.binary.read_bytes()).hexdigest(),'test_source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'scope':'Real libcanberra and private PipeWire/PulseAudio virtual sink monitor; no physical devices or host audio graph'},indent=2)+'\n')
