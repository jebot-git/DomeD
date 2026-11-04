#!/usr/bin/env python3
"""Real two-client X11 clipboard protocol on a caller-selected private Xvfb."""
from pathlib import Path
import json,os,select,signal,subprocess,sys,tempfile,time
from Xlib import X,Xatom,XK,display,error
from Xlib.ext import xtest
from Xlib.protocol import event
signal.pthread_sigmask(signal.SIG_SETMASK,[])
binary=Path(sys.argv[1]).resolve();out=Path(sys.argv[2]).resolve();out.mkdir(parents=True,exist_ok=True)
with tempfile.TemporaryDirectory(prefix='dome-clipboard-') as home:
    rd,wr=os.pipe();server=subprocess.Popen(['Xvfb','-displayfd',str(wr),'-screen','0','800x600x24','-nolisten','tcp'],pass_fds=(wr,),stdout=subprocess.DEVNULL,stderr=(out/'xvfb.log').open('w'));os.close(wr)
    try:
        assert select.select([rd],[],[],20)[0],'Private Xvfb did not report display'
        with os.fdopen(rd) as pipe:screen=':'+pipe.readline().strip()
        d=display.Display(screen);owner=d.screen().root.create_window(0,0,1,1,0,X.CopyFromParent);clipboard=d.intern_atom('CLIPBOARD');utf8=d.intern_atom('UTF8_STRING');targets=d.intern_atom('TARGETS');marker=d.intern_atom('DOME_CLIPBOARD_PRESERVED_EVENT');results=[]
        for mode in ['normal','malformed','refuse','late','ownership']:
            run=out/mode;run.mkdir();owner.set_selection_owner(clipboard,X.CurrentTime);d.sync();env={**os.environ,'DISPLAY':screen,'FLTK_BACKEND':'x11','HOME':home+'/'+mode,'XDG_CONFIG_HOME':home+'/'+mode+'/config','XDG_CACHE_HOME':home+'/'+mode+'/cache','LIBGL_ALWAYS_SOFTWARE':'1'};env.pop('WAYLAND_DISPLAY',None);env.pop('WAYLAND_SOCKET',None)
            start=time.monotonic();app=subprocess.Popen([str(binary),mode,str(run),str(owner.id)],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT);os.set_blocking(app.stdout.fileno(),False);text='';armed=False;reclaimed=False;image_reclaimed=False;pending=[];requests=[];window=None
            def respond(request):
                prop=request.property or request.target
                if request.target==targets:
                    if mode=='refuse':prop=X.NONE
                    elif mode=='malformed':request.requestor.change_property(prop,utf8,8,b'wrong TARGETS type')
                    else:request.requestor.change_property(prop,Xatom.ATOM,32,[utf8,Xatom.STRING,targets])
                elif request.target in [utf8,Xatom.STRING]:request.requestor.change_property(prop,request.target,8,b'external exact payload')
                else:prop=X.NONE
                request.requestor.send_event(event.SelectionNotify(time=request.time,requestor=request.requestor,selection=request.selection,target=request.target,property=prop),propagate=False);d.flush()
            try:
                while app.poll() is None:
                    assert time.monotonic()-start<180,'Native protocol client timeout'
                    select.select([d.fileno(),app.stdout.fileno()],[],[],.01)
                    try:
                        block=os.read(app.stdout.fileno(),65536)
                        if block:text+=block.decode(errors='replace')
                    except BlockingIOError:pass
                    if 'READY\n' in text and not armed:
                        window=next((w for w in d.screen().root.query_tree().children if w.get_wm_name()=='Clipboard protocol probe'),None);assert window is not None;window.set_input_focus(X.RevertToParent,X.CurrentTime);code=d.keysym_to_keycode(XK.string_to_keysym('F1'));xtest.fake_input(d,X.KeyPress,code);xtest.fake_input(d,X.KeyRelease,code);d.sync();time.sleep(.05);owner.set_selection_owner(clipboard,X.CurrentTime);d.sync();(run/'armed').write_text('external newer owner\n');armed=True
                    if 'RECLAIM\n' in text and not reclaimed:
                        owner.set_selection_owner(clipboard,X.CurrentTime);d.sync();(run/'reclaimed').write_text('external new owner\n');reclaimed=True
                    if 'RECLAIM_IMAGE\n' in text and not image_reclaimed:
                        owner.set_selection_owner(clipboard,X.CurrentTime);d.sync();(run/'image-reclaimed').write_text('external image owner\n');image_reclaimed=True
                    while d.pending_events():
                        request=d.next_event()
                        if request.type!=X.SelectionRequest:continue
                        requests.append({'target':d.get_atom_name(request.target),'requestor':request.requestor.id,'time':request.time})
                        if request.target==targets:
                            assert window is not None
                            window.send_event(event.ClientMessage(window=window,client_type=marker,data=(32,[len(requests),0,0,0,0])),propagate=False);d.flush()
                        if mode=='late':pending.append((time.monotonic()+.4,request))
                        else:respond(request)
                    for due,request in list(pending):
                        if time.monotonic()>=due:respond(request);pending.remove((due,request))
                app.wait()
                while True:
                    try:block=os.read(app.stdout.fileno(),65536)
                    except BlockingIOError:break
                    if not block:break
                    text+=block.decode(errors='replace')
            finally:
                if app.poll() is None:app.terminate();app.wait(timeout=10)
                (run/'app.log').write_text(text)
            result={'mode':mode,'exit':app.returncode,'seconds':time.monotonic()-start,'requests':requests};results.append(result);(out/'report.json').write_text(json.dumps(results,indent=2)+'\n');print(result,flush=True);assert app.returncode==0 and 'PASS clipboard '+mode in text,text[-7000:]
        owner.destroy();d.close()
    finally:
        server.terminate()
        try:server.wait(timeout=5)
        except subprocess.TimeoutExpired:server.kill();server.wait()
