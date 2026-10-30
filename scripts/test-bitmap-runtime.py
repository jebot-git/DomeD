#!/usr/bin/env python3
"""Verify native BDF/PCF coverage, shaping, metrics and exact Pango/Cairo pixels."""
import argparse,hashlib,json,os,pathlib,shlex,subprocess,tempfile
from xml.sax.saxutils import escape
ROOT=pathlib.Path(__file__).resolve().parents[1]
def run(command,**kwargs):
    r=subprocess.run([str(s) for s in command],capture_output=True,text=True,**kwargs)
    if r.returncode:raise RuntimeError(' '.join(map(str,command))+'\n'+r.stdout+r.stderr)
    return r.stdout

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--prefix',type=pathlib.Path,required=True);p.add_argument('--pcf',type=pathlib.Path,required=True);p.add_argument('--outline',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path)
    a=p.parse_args();prefix=a.prefix.resolve();env={**os.environ,'LD_LIBRARY_PATH':str(prefix/'lib'),'PKG_CONFIG_PATH':str(prefix/'lib/pkgconfig')}
    loaded=json.loads(run(['python3',ROOT/'scripts/check-bitmap-runtime.py','--manifest',prefix/'share/domed-bdf-runtime/manifest.json'],env=env))
    fonts=[ROOT/'assets/fonts/unifont-17.0.04.bdf',ROOT/'assets/fonts/hicchicc--maruminya/MaruMinya.bdf',a.pcf.resolve()]
    assert all(f.is_file() for f in [a.outline,*fonts])
    with tempfile.TemporaryDirectory(prefix='domed-bitmap-test-') as folder:
        tmp=pathlib.Path(folder)
        flags=shlex.split(run(['pkg-config','--cflags','--libs','harfbuzz','freetype2'],env=env))
        run([*shlex.split(os.environ.get('CXX','c++')),'-std=c++17',ROOT/'native/harfbuzz-bdf/test-bdf.cc','-o',tmp/'test-native',*flags],env=env)
        native=run([tmp/'test-native',a.outline,*fonts],env=env);print(native,end='')
        pixels=[]
        for i,font in enumerate(fonts):
            fixture=tmp/str(i);fixture.mkdir();(fixture/font.name).symlink_to(font)
            conf=fixture/'fonts.conf';conf.write_text('<fontconfig><dir>'+escape(str(fixture))+'</dir><cachedir>'+escape(str(fixture/'cache'))+'</cachedir></fontconfig>')
            # Each font gets a fresh process/config, so equal BDF/PCF family names
            # cannot accidentally validate the other file's pixels.
            isolated={**env,'FONTCONFIG_FILE':str(conf)}
            text=run(['uv','run','--no-project','--with','freetype-py','python',ROOT/'scripts/test-pango-bdf.py',font],env=isolated);pixels.append(text);print(text,end='')
        adapter_flags=shlex.split(run(['pkg-config','--cflags','--libs','freetype2','fontconfig'],env=env))
        run([*shlex.split(os.environ.get('CXX','c++')),'-std=c++17',ROOT/'native/bitmap-runtime/test-pcf-adapter.cpp','-o',tmp/'test-adapter',*adapter_flags,'-lz'],env=env)
        adapter=run([tmp/'test-adapter',a.pcf.resolve(),tmp/'adapted.bdf'],env={**env,'XDG_CACHE_HOME':str(tmp/'cache')});print(adapter,end='')
        report=dict(passed=True,native=native,pixels=pixels,loaded=loaded,pcf_adapter=adapter)
        if a.output:a.output.write_text(json.dumps(report,indent=2)+'\n')
if __name__=='__main__':main()
