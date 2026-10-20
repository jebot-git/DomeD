#!/usr/bin/env python3
"""Read-only live smoke test. Snapshots are not per-frame latency percentiles."""
import argparse,json,time,pathlib,statistics,os
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--seconds',type=float,default=60)
parser.add_argument('--feed',type=pathlib.Path,default=pathlib.Path(os.environ.get('XDG_RUNTIME_DIR',f'/run/user/{os.getuid()}'))/'wivrn-pro/performance.json')
parser.add_argument('--output',type=pathlib.Path,default=pathlib.Path('artifacts/xr-performance/live.json'))
args=parser.parse_args()
if not 1 <= args.seconds <= 600:parser.error('--seconds must be 1..600')
out=args.output;out.parent.mkdir(parents=True,exist_ok=True); samples=[]; start=time.monotonic(); ticks=os.sysconf('SC_CLK_TCK')
pids=[];names={}
for proc in pathlib.Path('/proc').iterdir():
 if not proc.name.isdigit():continue
 try:
  argv=proc.joinpath('cmdline').read_bytes().split(b'\0')
  name=pathlib.Path(os.fsdecode(argv[0])).name
  if name in ('dome-compositor','dome-xr','wivrn-pro-server') and b'--shora-worker' not in argv:
   pid=int(proc.name);pids.append(pid);names[str(pid)]=name
 except (OSError,ValueError):pass
def cpu(pid):
 try:
  s=pathlib.Path(f'/proc/{pid}/stat').read_text().split(') ',1)[1].split();return (int(s[11])+int(s[12]))/ticks
 except (OSError,ValueError):return None
first={p:cpu(p) for p in pids}
while time.monotonic()-start<args.seconds:
 try:
  d=json.loads(args.feed.read_text());now=time.monotonic_ns()
  samples.append({'elapsed':time.monotonic()-start,'age_ms':(now-d['updated_ns'])/1e6,'target_hz':d['target_hz'],'apps':d['apps'],'streams':d['streams']})
 except (OSError,ValueError,KeyError):pass
 time.sleep(.25)
seconds=time.monotonic()-start
if not samples:raise SystemExit('No performance feed samples; check --feed and the WiVRn session.')
def stats(v):
 v=sorted(v);return {'p50':statistics.median(v),'p95':v[min(len(v)-1,int(.95*len(v)))],'max':max(v)} if v else None
summary={'process_names':names,'seconds':seconds,'samples':len(samples),'feed_age_ms':stats([s['age_ms'] for s in samples]),'target_hz':sorted(set(s['target_hz'] for s in samples)), 'process_cpu_percent':{str(p):100*(cpu(p)-first[p])/seconds for p in pids if first[p] is not None and cpu(p) is not None},'apps':{},'streams':{}}
for key,fields in [('apps',['cpu_ms','gpu_ms','interval_ms','period_ms']),('streams',['encode_ms','decode_ms','delivery_ms','encode_display_ms'])]:
 for ident in sorted(set(a['id'] for s in samples for a in s[key])):
  rows=[a for s in samples for a in s[key] if a['id']==ident and a['updated_ns'] and (start+s['elapsed'])*1e9-a['updated_ns']<2e9]
  summary[key][str(ident)]={f:stats([a[f] for a in rows]) for f in fields}
  if key=='streams' and rows:
   summary[key][str(ident)]['presented_delta']=rows[-1].get('presented_frames',0)-rows[0].get('presented_frames',0)
   summary[key][str(ident)]['repeat_delta']=rows[-1].get('repeats',0)-rows[0].get('repeats',0)
out.write_text(json.dumps({'scope':'Uncontrolled live workload; application GPU timers are not runtime layer GPU time. Stream values are snapshots, not per-frame percentiles.','summary':summary,'samples':samples},indent=2)+'\n');print(json.dumps(summary,indent=2))
