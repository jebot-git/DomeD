#!/usr/bin/env python3
"""Extract downloaded i686 development RPMs into a user build SDK; no system writes."""
import argparse,pathlib,re,subprocess,os
P=pathlib.Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('--archives',type=P,required=True);p.add_argument('--output',type=P,required=True);a=p.parse_args()
if os.geteuid()==0:p.error('Run as the regular user')
root=a.output.resolve();root.mkdir(parents=True,exist_ok=True)
for rpm in sorted(a.archives.glob('*.rpm')):
    proc=subprocess.Popen(['rpm2cpio',str(rpm.resolve())],stdout=subprocess.PIPE)
    result=subprocess.run(['cpio','-idm','--quiet'],cwd=root,stdin=proc.stdout,capture_output=True);proc.stdout.close()
    assert proc.wait()==0 and result.returncode==0,result.stderr
for pc in (root/'usr/lib/pkgconfig').glob('*.pc'):
    t=pc.read_text().replace('prefix=/usr','prefix='+str(root/'usr'))
    t=t.replace('libdir=/usr/lib','libdir='+str(root/'usr/lib')).replace('includedir=/usr/include','includedir='+str(root/'usr/include'))
    pc.write_text(t)
for file in root.rglob('*'):
    if file.is_symlink() and not file.exists():
        target=file.readlink();host=target if target.is_absolute() else P('/usr/lib')/(file.parent/target).name
        if host.exists():file.unlink();file.symlink_to(host)
libc=root/'usr/lib/libc.so'
if libc.exists() and not libc.is_symlink():
    libc.write_text(re.sub(r'\S*usr/lib/libc_nonshared\.a',str(root/'usr/lib/libc_nonshared.a'),libc.read_text()))
print('Prepared user-only i686 SDK:',root)
