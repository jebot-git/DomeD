#!/usr/bin/env python3
"""Private PipeWire graph: real system/microphone capture, Opus, A/V timing."""
import array,json,math,os,signal,struct,subprocess,sys,tempfile,time,wave
from pathlib import Path
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get('DOME_CAPTURE_AUDIO_TEST_BUS')!='1':
    os.execvpe('dbus-run-session',['dbus-run-session','--',sys.executable,*sys.argv],
              {**os.environ,'DOME_CAPTURE_AUDIO_TEST_BUS':'1'})
root=Path(__file__).resolve().parents[1];out=root/'artifacts/capture/av/audio'
out.mkdir(parents=True,exist_ok=True);processes=[];logs=[];checks=[]
def check(ok,name):
    assert ok,name
    print('PASS '+name,flush=True);checks.append(name)
def start(args,name,env):
    log=(out/(name+'.log')).open('w');logs.append(log)
    p=subprocess.Popen(list(map(str,args)),cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT);processes.append(p);return p
def wait(fn,timeout=15):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        try:
            v=fn()
            if v:return v
        except (OSError,ValueError,KeyError,StopIteration):pass
        time.sleep(.04)
    raise AssertionError('Timed out; see artifacts/capture/av/audio')
try:
    with tempfile.TemporaryDirectory(prefix='dome-capture-audio-')as directory:
        runtime=Path(directory);runtime.chmod(0o700)
        env={**os.environ,'XDG_RUNTIME_DIR':directory,'PIPEWIRE_RUNTIME_DIR':directory,'PIPEWIRE_REMOTE':'pipewire-0',
             'HOME':directory,'XDG_CONFIG_HOME':directory+'/config','XDG_STATE_HOME':directory+'/state','XDG_DATA_HOME':directory+'/data',
             'XDG_DATA_DIRS':directory+'/empty','DBUS_SYSTEM_BUS_ADDRESS':'unix:path='+directory+'/no-system-bus'}
        start(['pipewire','-c',root/'scripts/audio-test.conf'],'pipewire',env)
        wait(lambda:(runtime/'pipewire-0').is_socket())
        start(['wireplumber','--profile','policy'],'policy',env)
        def command(*args):return subprocess.check_output(args,env=env,stderr=subprocess.DEVNULL,timeout=5)
        def nodes():return {o['info']['props']['node.name']:o for o in json.loads(command('pw-dump'))if o.get('type')=='PipeWire:Interface:Node'}
        def seed():
            n=nodes()
            if 'dome.test.speakers'not in n or 'dome.test.mic'not in n:return False
            command('wpctl','set-default',str(n['dome.test.speakers']['id']))
            command('wpctl','set-default',str(n['dome.test.mic']['id']))
            return True
        wait(seed)
        check(all(name.startswith('dome.test.')for name in nodes()),'Audio graph has only isolated virtual devices')
        for name,hz,target in [('system',440,'dome.test.speakers'),('microphone',880,'dome.test.mic')]:
            wav=runtime/(name+'.wav')
            with wave.open(str(wav),'wb')as w:
                w.setnchannels(2);w.setsampwidth(2);w.setframerate(48000)
                pcm=b''.join(struct.pack('<hh',*([int(8000*math.sin(2*math.pi*hz*i/48000))]*2))for i in range(48000*9))
                w.writeframes(pcm)
            if name=='system':start(['pw-play','--target',target,wav],name+'-tone',env)
            else:
                # A null source's monitor needs a direct link. Letting policy
                # autoconnect playback to an Audio/Source falls back to the
                # default sink and contaminates the system-output test.
                start(['pw-play','--target','0','-P','{ node.name = dome.capture.test.mic-tone }',wav],name+'-tone',env)
                wait(lambda:'dome.capture.test.mic-tone' in nodes())
                def link_mic():
                    ports=command('pw-link','-i').decode().splitlines()
                    target=next(p for p in ports if p.startswith('dome.test.mic:'))
                    source=next(p for p in command('pw-link','-o').decode().splitlines()if p.startswith('dome.capture.test.mic-tone:'))
                    command('pw-link',source,target);return True
                wait(link_mic)
        for mode in ['system','microphone','off']:
            video=out/(mode+'.mkv');report=out/(mode+'.json');video.unlink(missing_ok=True);report.unlink(missing_ok=True)
            p=start([root/'build/capture/dome-capture','--record','--source','test','--encoder','qsv','--audio',mode,
                     '--seconds','1.2','--output',video,'--report',report],mode,env)
            wait(lambda:p.poll()is not None,20);result=json.loads(report.read_text())
            check(p.returncode==0 and 'error'not in result,mode+': capture finalizes successfully')
            probe=json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-show_format','-of','json',video]))
            a=[s for s in probe['streams']if s['codec_type']=='audio'];v=probe['streams'][0]
            if mode=='off':check(not a,'Audio off creates no audio stream');continue
            check(len(a)==1 and a[0]['codec_name']=='opus' and a[0]['channels']==2,mode+': stereo Opus audio track is present')
            raw=subprocess.check_output(['ffmpeg','-v','error','-i',video,'-map','0:a:0','-ac','1','-ar','48000','-f','f32le','-'])
            samples=array.array('f');samples.frombytes(raw)
            middle=samples[4800:43200]
            crossings=sum(middle[i-1]<0<=middle[i]for i in range(1,len(middle)))
            hz=crossings/(len(middle)/48000)
            check(abs(hz-(440 if mode=='system'else 880))<8,mode+': decoded tone comes from the intended device')
            check(abs(float(a[0]['start_time'])-float(v['start_time']))<.1 and
                  abs(len(samples)/48000-float(probe['format']['duration']))<.12,
                  mode+': audio and video have aligned start and duration')
        # Dropping the graph must surface an error and still finalize captured packets.
        video=out/'disconnect.mkv';report=out/'disconnect.json';video.unlink(missing_ok=True);report.unlink(missing_ok=True)
        p=start([root/'build/capture/dome-capture','--record','--source','test','--encoder','qsv','--audio','system',
                 '--seconds','20','--output',video,'--report',report],'disconnect',env)
        wait(lambda:video.exists());time.sleep(.3);processes[0].terminate()
        wait(lambda:p.poll()is not None)
        result=json.loads(report.read_text())
        check(p.returncode!=0 and result.get('error') and 'finalize_error'not in result,'Audio server loss reports an error and finalizes the recording')
finally:
    for p in reversed(processes):
        if p.poll()is None:p.terminate()
    for p in reversed(processes):
        try:p.wait(timeout=4)
        except subprocess.TimeoutExpired:p.kill();p.wait()
    for f in logs:f.close()
    (out/'checks.json').write_text(json.dumps(checks,indent=2)+'\n')
