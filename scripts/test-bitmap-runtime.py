#!/usr/bin/env python3
"""Test the native BDF fork, Pango pixels, and rejection of disguised outlines."""
import argparse, hashlib, json, os, pathlib, shlex, shutil, subprocess, tempfile
from xml.sax.saxutils import escape
ROOT = pathlib.Path(__file__).resolve().parents[1]

def run(command, **kwargs):
    result = subprocess.run([str(s) for s in command], text=True, capture_output=True, **kwargs)
    if result.returncode:
        raise RuntimeError(' '.join(map(str, command)) + '\n' + result.stdout + result.stderr)
    return result.stdout

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--prefix', type=pathlib.Path, default=ROOT / 'build/bitmap-runtime')
    p.add_argument('--outline', type=pathlib.Path, default=pathlib.Path('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'))
    args = p.parse_args(); prefix = args.prefix.expanduser().resolve()
    if not args.outline.is_file(): p.error('Supply a real TTF/OTF fixture with --outline')
    manifest = json.loads((prefix / 'share/domed-bdf-runtime/manifest.json').read_text())
    assert manifest['policy'] == 'native-unicode-bdf-only'
    libdir = prefix / 'lib'
    if not (libdir / 'libharfbuzz.so.0').is_file():
        libdir = pathlib.Path(run(['pkg-config', '--variable=libdir', 'harfbuzz']).strip())
    for name, checksum in manifest['libraries'].items():
        assert hashlib.sha256((libdir / name).read_bytes()).hexdigest() == checksum, name
    for key, name in [('harfbuzz_patch_sha256', 'harfbuzz-12.3.2-bdf-only.patch'),
                      ('pango_patch_sha256', 'pango-1.57.0-bitmap-only.patch')]:
        assert manifest[key] == hashlib.sha256((ROOT / 'native/harfbuzz-bdf' / name).read_bytes()).hexdigest()
    env = dict(os.environ, LD_LIBRARY_PATH=str(libdir), PKG_CONFIG_PATH=str(prefix / 'lib/pkgconfig'))
    fonts = [ROOT.parent / 'flterm-xr/xr/fonts' / n for n in ['cozette-13.bdf', 'scientifica-11.bdf', 'unifont-17.0.04.bdf']]
    if not all(f.is_file() for f in fonts):
        fonts = [ROOT / 'assets/fonts' / n for n in ['unifont-17.0.04.bdf', 'hicchicc--maruminya/MaruMinya.bdf', 'TakWolf--jelly-pixel-font/jelly-pixel-12px-monospaced-mixed.bdf']]
    with tempfile.TemporaryDirectory(prefix='domed-bdf-test-') as tmp:
        temp = pathlib.Path(tmp)
        flags = shlex.split(run(['pkg-config', '--cflags', '--libs', 'harfbuzz', 'freetype2'], env=env))
        run([*shlex.split(os.environ.get('CXX', 'c++')), '-std=c++17',
             ROOT / 'native/harfbuzz-bdf/test-bdf.cc', '-o', temp / 'test-bdf', *flags], env=env)
        direct = run([temp / 'test-bdf', args.outline, *fonts], env=env)
        print(direct, end='')
        fixture = temp / 'fonts'; fixture.mkdir()
        for font in fonts: shutil.copy2(font, fixture / font.name)
        shutil.copy2(args.outline, fixture / 'outline-renamed.bdf')
        conf = temp / 'fonts.conf'
        conf.write_text('<?xml version="1.0"?><!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd"><fontconfig>'
                        '<dir>' + escape(str(fixture)) + '</dir><cachedir>' + escape(str(temp / 'cache')) + '</cachedir>'
                        '<selectfont><rejectfont><pattern/></rejectfont><acceptfont><pattern>'
                        '<patelt name="fontformat"><string>BDF</string></patelt>'
                        '<patelt name="scalable"><bool>false</bool></patelt>'
                        '</pattern></acceptfont></selectfont></fontconfig>')
        env['FONTCONFIG_FILE'] = str(conf)
        faces = run(['fc-list', '-f', '%{file}|%{fontformat}|%{scalable}\n'], env=env)
        assert len(faces.splitlines()) == len(fonts), faces
        assert all(line.endswith('|BDF|False') for line in faces.splitlines()), faces
        pixel = run(['uv', 'run', '--no-project', '--with', 'freetype-py', 'python',
                     ROOT / 'scripts/test-pango-bdf.py', *[fixture / f.name for f in fonts]], env=env)
        print(pixel, end='')
        loaded = json.loads(run(['python3', '-c', '''import ctypes,json,pathlib
ctypes.CDLL('libpango-1.0.so.0'); ctypes.CDLL('libpangocairo-1.0.so.0')
print(json.dumps(sorted({l.split()[-1] for l in pathlib.Path('/proc/self/maps').read_text().splitlines() if 'libpango' in l or 'libharfbuzz' in l})))'''], env=env))
        assert len(loaded) == 4 and all(pathlib.Path(path).is_relative_to(prefix) for path in loaded), loaded
        # Exercise the actual session export implementation, including all bundled
        # fonts, without rebuilding the unrelated compositor/graphics stack.
        rust = temp / 'font-export.rs'
        rust.write_text('#[path=' + json.dumps(str(ROOT / 'src/bundled_fonts.rs')) + '] mod bundled_fonts;\n' +
                        '#[path=' + json.dumps(str(ROOT / 'src/font_export.rs')) + '] mod font_export;\n')
        run(['rustc', '--edition=2021', '--test', rust, '-o', temp / 'font-export-test'])
        font_export = run([temp / 'font-export-test', '--nocapture'])
        print(font_export, end='')
        output = ROOT / 'build/harfbuzz-bdf/test-results.json'
        output.write_text(json.dumps(dict(status='passed',direct=direct,pango_pixels=pixel,
                                         fonts=faces,loaded_libraries=loaded,
                                         font_export=font_export), indent=2) + '\n')
        print('PASS private loader paths and rejection of outline-renamed.bdf')

if __name__ == '__main__': main()
