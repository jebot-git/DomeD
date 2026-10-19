#!/usr/bin/env python3
"""Exercise the development API; starts and stops its own layout server."""
import json,math,os,socket,subprocess,time,urllib.request,urllib.error
from pathlib import Path
root=Path(__file__).resolve().parents[1]
with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
env={**os.environ,'DOME_PREVIEW_PORT':str(port)}
p=subprocess.Popen([str(root/'target/debug/dome-preview')],env=env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
base=f'http://127.0.0.1:{port}'
def get():return json.load(urllib.request.urlopen(base+'/api/state'))['scene']
def post(value,**headers):
    request=urllib.request.Request(base+'/api/action',data=json.dumps(value).encode(),headers={'Content-Type':'application/json',**headers})
    try:
        with urllib.request.urlopen(request) as r:return r.status,json.load(r)
    except urllib.error.HTTPError as e:return e.code,json.loads(e.read()) if e.code!=403 else {}
try:
    for _ in range(100):
        try:get();break
        except OSError:time.sleep(.03)
    initial=get();count=len(initial['windows'])
    xr=json.load(urllib.request.urlopen(base+'/api/xr-scene'))
    assert xr['source']=='preview' and len(xr['panels'])==count
    for panel in xr['panels']:
        assert len(panel['vertices'])==98
        assert all(len(v)==3 and all(math.isfinite(x)for x in v)for v in panel['vertices'])

    assert post({'op':'add','title':'API smoke','app_id':'Test'})[0]==200
    s=get();assert len(s['windows'])==count+1;id=s['focused']
    assert post({'op':'minimize','id':id})[0]==200
    assert next(w for w in get()['windows']if w['id']==id)['minimized']
    assert post({'op':'focus','id':id})[0]==200
    assert post({'op':'close','id':id})[0]==200
    assert len(get()['windows'])==count
    assert post({'op':'add_workspace','name':'Review'})[0]==200
    assert post({'op':'rename_workspace','workspace':3,'name':'Review renamed'})[0]==200
    assert get()['workspaces'][3]['name']=='Review renamed'
    assert post({'op':'remove_workspace','workspace':3,'move_to':0})[0]==200
    assert post({'op':'configure','config':{'radius_m':-1}})[0]==400
    assert post({'op':'add','title':'x','app_id':'Test'},Origin='http://example.test')[0]==403
    assert post({'op':'add','title':'x','app_id':'Test'},Host='example.test')[0]==403
    assert post({'op':'add','title':'x','app_id':'Test','unexpected':True})[0]==400
    print('API: window and desktop lifecycle, validation and same-origin checks passed')
finally:
    p.terminate();p.wait(timeout=3)
