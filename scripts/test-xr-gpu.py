#!/usr/bin/env python3
"""Private Xvfb integration: GPU leases, pool reuse, real Qt fractional buffers,
and higher-density fallback pixels. Requires PySide6 and a DRM render node.
Run after cargo build --features nested,xr,standalone.
"""
import array, json, os, socket, struct, subprocess, sys, tempfile, time, urllib.request
from pathlib import Path
root = Path(__file__).resolve().parents[1]
if '--client' in sys.argv:
    from PySide6.QtWidgets import QApplication, QWidget
    from PySide6.QtGui import QPainter, QColor
    from PySide6.QtCore import QTimer
    app = QApplication([]); app.setApplicationName('dome-density-fixture'); app.setDesktopFileName('dome-static-fixture' if '--static' in sys.argv else 'dome-density-fixture')
    journal = Path(sys.argv[-1])
    class Client(QWidget):
        def paintEvent(self, event):
            p = QPainter(self); p.fillRect(self.rect(), QColor('#4080bf'))
            p.fillRect(0, 0, self.width(), self.height() // 4, QColor('red'))
            p.fillRect(0, 3 * self.height() // 4, self.width(), self.height(), QColor('green'))
            dpr = self.devicePixelRatioF(); p.scale(1/dpr, 1/dpr)
            for x in range(int(self.width()*dpr)):
                p.fillRect(x, int(self.height()*dpr/2), 1, 4, QColor('white' if x%2 else 'black'))
        def record(self):
            journal.write_text(json.dumps({'width':self.width(), 'height':self.height(), 'dpr':self.devicePixelRatioF()}))
            if '--static' not in sys.argv:self.update()
    widget = Client(); widget.resize(720,460); widget.setWindowTitle('XR density fixture'); widget.show()
    timer=QTimer();timer.timeout.connect(widget.record);timer.start(100)
    raise SystemExit(app.exec())
if os.environ.get('DOME_GPU_TEST_BUS') != '1':
    os.execvpe('dbus-run-session', ['dbus-run-session','--',sys.executable,*sys.argv], {**os.environ,'DOME_GPU_TEST_BUS':'1'})

with tempfile.TemporaryDirectory(prefix='dome-gpu-') as directory:
    runtime=Path(directory);runtime.chmod(0o700)
    children=[]; logs=[]; leases=[]
    output=root/'artifacts/xr-gpu';output.mkdir(parents=True,exist_ok=True)
    def start(args,name,env,**kwargs):
        log=(output/(name+'.log')).open('w');logs.append(log)
        p=subprocess.Popen(list(map(str,args)),env=env,stdout=log,stderr=subprocess.STDOUT,**kwargs);children.append(p);return p
    def wait(fn, timeout=15):
        end=time.monotonic()+timeout;last=None
        while time.monotonic()<end:
            try:
                result=fn()
                if result:return result
            except (OSError,ValueError,KeyError,StopIteration) as e:last=e
            if any(p.poll() is not None for p in children):raise AssertionError('Child exited; see artifacts/xr-gpu')
            time.sleep(.03)
        raise AssertionError('Timed out: '+str(last))
    def exact(s,n):
        b=b''
        while len(b)<n:
            more=s.recv(n-len(b))
            if not more:raise OSError('GPU socket closed')
            b+=more
        return b
    def gpu_frame(path,id):
        s=socket.socket(socket.AF_UNIX);s.settimeout(2);s.connect(path);s.sendall(struct.pack('<Q',id))
        byte,anc,flags,_=s.recvmsg(1,socket.CMSG_SPACE(16))
        fds=array.array('i')
        for level,kind,data in anc:
            if level==socket.SOL_SOCKET and kind==socket.SCM_RIGHTS:fds.frombytes(data)
        if byte!=b'\x01':s.close();return None
        assert len(fds)==1 and not flags&socket.MSG_CTRUNC
        info=json.loads(exact(s,struct.unpack('<I',exact(s,4))[0]))
        result=(s,fds[0],info);leases.append(result);return result
    def release(f):
        f[0].close();os.close(f[1]);leases.remove(f)
    try:
        xvfb=root/'.local-deps/root/usr/bin/Xvfb'
        if not xvfb.exists():xvfb='Xvfb'
        rd,wr=os.pipe();start([xvfb,'-displayfd',wr,'-screen','0','1400x1000x24','-nolisten','tcp'],'xvfb',os.environ.copy(),pass_fds=(wr,));os.close(wr)
        with os.fdopen(rd) as pipe:display=':'+pipe.readline().strip()
        with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        base=f'http://127.0.0.1:{port}'
        http=urllib.request.build_opener(urllib.request.ProxyHandler({}))
        def request(path,data=None):
            return http.open(urllib.request.Request(base+path,data=None if data is None else json.dumps(data).encode(),headers={'Content-Type':'application/json'}),timeout=2)
        def state():
            with request('/api/state') as r:return json.load(r)['scene']
        env={**os.environ,'DISPLAY':display,'XDG_RUNTIME_DIR':directory,'XDG_CONFIG_HOME':str(runtime/'config'),
             'XDG_DATA_HOME':str(runtime/'data'),'XDG_STATE_HOME':str(runtime/'state'),'DOME_PREVIEW_PORT':str(port),
             'DOME_START_XR':'0','DOME_RENDERER':'vulkan','RUST_LOG':'warn','XR_RUNTIME_JSON':str(runtime/'no-openxr.json')}
        for key in ('WAYLAND_DISPLAY','WAYLAND_SOCKET','LIBGL_ALWAYS_SOFTWARE','GALLIUM_DRIVER','MESA_LOADER_DRIVER_OVERRIDE'):env.pop(key,None)
        start([os.environ.get('DOME_TEST_COMPOSITOR',str(root/'target/debug/dome-compositor')),'--backend','nested'],'compositor',env);wait(state)
        xr=next(p for p in runtime.glob('*.dome-session.json') if json.loads(p.read_text())['realm']=='xr')
        client_env={**env,'WAYLAND_DISPLAY':str(xr).removesuffix('.dome-session.json'),'QT_QPA_PLATFORM':'wayland','WAYLAND_DEBUG':'1'}
        journal=runtime/'client.json';start([sys.executable,__file__,'--client',journal],'client',client_env)
        window=wait(lambda:next(w for w in state()['windows'] if w['app_id']=='dome-density-fixture'));id=window['id']
        client=wait(lambda:json.loads(journal.read_text()) if journal.exists() else None)
        client=wait(lambda:(v if abs(v['dpr']-1.5)<.01 else None) if (v:=json.loads(journal.read_text())) else None)
        assert (client['width'],client['height'])==(720,460),client
        with request('/api/xr-control') as r:path=json.load(r)['gpu_socket']
        assert path and (os.stat(path).st_mode&0o777)==0o600
        first=wait(lambda:gpu_frame(path,id));info=first[2]
        assert (info['width'],info['height'])==(1080,690),info
        assert (info['logical_width'],info['logical_height'])==(720,460),info
        def next_frame(previous):
            f=gpu_frame(path,id)
            if f and f[2]['serial']!=previous[2]['serial']:return f
            if f:release(f)
        second=wait(lambda:next_frame(first));third=wait(lambda:next_frame(second))
        time.sleep(.2);same=gpu_frame(path,id);assert same[2]['serial']==third[2]['serial'],'Leased pool was overwritten';release(same)
        release(first);fourth=wait(lambda:next_frame(third))
        assert fourth[2]['serial']>third[2]['serial']
        for f in list(leases):release(f)
        # An idle publication must not monopolize capture scheduling while a
        # second application continues committing new buffers.
        static_journal=runtime/'static.json'
        start([sys.executable,__file__,'--client','--static',static_journal],'static-client',client_env)
        static_id=wait(lambda:next((w['id'] for w in state()['windows'] if w['app_id']=='dome-static-fixture'),None))
        idle=wait(lambda:gpu_frame(path,static_id));release(idle)
        serials=set();end=time.monotonic()+1.2
        while time.monotonic()<end:
            for target in (static_id,id):
                f=gpu_frame(path,target)
                if f:
                    if target==id:serials.add(f[2]['serial'])
                    release(f)
            time.sleep(.02)
        assert len(serials)>=4,('Idle window starved changing window',serials)
        def cpu_frame():
            with request(f'/api/xr-frame/{id}') as r:
                return int(r.headers['X-Dome-Width']),int(r.headers['X-Dome-Height']),float(r.headers['X-Dome-Scale']),r.read()
        w,h,scale,pixels=wait(cpu_frame)
        assert (w,h,scale)==(1080,690,1.5),(w,h,scale)
        assert len(pixels)==w*h*4
        row=pixels[(h//2*w)*4:((h//2+1)*w)*4]
        transitions=sum(abs(row[x*4]-row[(x-1)*4])>150 for x in range(10,w-10))
        assert transitions>w*.8,('High-frequency detail was downsampled',transitions,w)
        config=state()['config'];config['xr_render_scale']=2.
        with request('/api/action',{'op':'configure','config':config}):pass
        wait(lambda:abs(json.loads(journal.read_text())['dpr']-2.)<.01)
        larger=wait(lambda:(f if f[2]['width']==1440 else (release(f) or None)) if (f:=gpu_frame(path,id)) else None)
        assert larger[2]['height']==920;release(larger)
        assert state()['windows'][0]['desktop']==window['desktop'],'Density changed logical geometry'
        print('PASS: GPU descriptors, leases, bounded pool reuse, 150–200% real Qt buffers and retained pixel detail, idle-window fairness')
    finally:
        for f in list(leases):release(f)
        for p in reversed(children):
            if p.poll() is None:p.terminate()
        for p in reversed(children):
            try:p.wait(timeout=4)
            except subprocess.TimeoutExpired:p.kill();p.wait()
        for log in logs:log.close()
