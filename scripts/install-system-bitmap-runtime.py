#!/usr/bin/python3
"""Install a verified BDF system package plan, policy and session guard."""
from pathlib import Path
import apt,datetime,fcntl,hashlib,json,os,re,shutil,subprocess,argparse
repo=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--plan-dir',type=Path,default=repo/'build/system-bdf-packages')
base=parser.parse_args().plan_dir.resolve()
if os.geteuid()!=0:raise SystemExit('Administrator authentication required.')
os.umask(0o022)
lock=open('/run/lock/domed-system-bdf-fork.lock','w');fcntl.flock(lock,fcntl.LOCK_EX)
p=json.loads((base/'plan.json').read_text());c=apt.Cache()
for item in p['packages']:assert c[item['name']].installed.version==item['version'],item['name']
for a in p['archives']:assert hashlib.sha256(Path(a['path']).read_bytes()).hexdigest()==a['sha256']
source=(repo/'packaging/dome-session').read_text();a=source.index('# Only the native BDF fork');b=source.index('export DOME_LOGIN_SESSION',a);block=source[a:b]
session=Path('/usr/local/bin/dome-session');old=session.read_text();anchor='export DOME_LOGIN_SESSION=1 DOME_BACKEND=drm\n'
assert old.count(anchor)==1
if '# Only the native BDF fork' in old:
 start=old.index('# Only the native BDF fork');end=old.index(anchor,start)
 new=old[:start]+block+old[end:]
else:new=old.replace(anchor,block+anchor)
(base/'installed-session.new').write_text(new)
subprocess.run(['bash','-n',str(base/'installed-session.new')],check=True)
cmd=['apt-get','--reinstall','install',*[i['path'] for i in p['archives']]]
r=subprocess.run([cmd[0],'-s',*cmd[1:]],text=True,capture_output=True,check=True)
(base/'authenticated-plan.log').write_text(r.stdout+r.stderr)
assert not re.findall(r'^(?:Remv|Purg) (\S+)',r.stdout,re.M)
assert set(re.findall(r'^Inst (\S+)',r.stdout,re.M))=={i['name'] for i in p['packages']}
backup=Path('/var/backups/de-xr')/('system-bdf-fork-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
backup.mkdir(mode=0o700);shutil.copytree(base/'rollback-packages',backup/'packages')
for path,name in [('/var/lib/dpkg/status','dpkg-status'),('/var/lib/apt/extended_states','apt-extended-states'),('/usr/local/bin/dome-session','dome-session')]:shutil.copy2(path,backup/name)
shutil.copy2(base/'plan.json',backup/'plan.json')
(backup/'manual-packages.txt').write_text(subprocess.check_output(['apt-mark','showmanual'],text=True))
result=dict(status='backup-complete',backup=str(backup));(base/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print('Rollback packages and session backup:',backup,flush=True)
with (base/'apply.log').open('w') as log:
 subprocess.run([cmd[0],'-y','--no-remove','-o','Dpkg::Options::=--force-confold',*cmd[1:]],check=True,stdout=log,stderr=subprocess.STDOUT,env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'})
 auto=[i['name'] for i in p['packages'] if not i['manual']]
 if auto:subprocess.run(['apt-mark','auto',*auto],check=True,stdout=log,stderr=subprocess.STDOUT)
 temp=session.with_name(session.name+'.bdf-new');temp.write_text(new);temp.chmod(session.stat().st_mode & 0o777);os.replace(temp,session)
 subprocess.run(['ldconfig'],check=True,stdout=log,stderr=subprocess.STDOUT)
 subprocess.run(['apt-get','check'],check=True,stdout=log,stderr=subprocess.STDOUT)
 clean={k:v for k,v in os.environ.items() if k not in ['LD_LIBRARY_PATH','LD_PRELOAD']}
 policy=subprocess.run(['/usr/local/libexec/domed/check-bitmap-runtime'],env=clean,text=True,capture_output=True,check=True)
 (base/'installed-policy.json').write_text(policy.stdout)
c=apt.Cache()
for item in p['packages']:
 assert c[item['name']].installed.version==p['versions'][item['source']],item['name']
 assert c[item['name']].is_auto_installed==(not item['manual']),item['name']
for item in p['new_libraries']:assert hashlib.sha256(Path(item['path']).read_bytes()).hexdigest()==item['sha256'],item['path']
for n in ['chatgpt','vrcx-0','fcitx5','fcitx5-anthy','fcitx5-hangul','wivrn-pro','qimgvr-native','domed-session-services','kwallet-xr']:assert c[n].is_installed,n
audit=subprocess.run(['dpkg','--audit'],text=True,capture_output=True,check=True);assert not audit.stdout.strip(),audit.stdout
for n in ['libharfbuzz0b','libpango-1.0-0']:
 assert c[n].candidate.version==c[n].installed.version,n
result.update(status='installed',versions=p['versions'],packages=[i['name'] for i in p['packages']],policy_check='passed',session_guard_installed=True,fork_only_apt_policy=True)
(base/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print('Installed system BDF-only HarfBuzz/Pango fork, required session guard and fork-only APT policy. No package removals.',flush=True)
