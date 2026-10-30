#!/usr/bin/env python3
"""Build the pinned BDF/PCF FreeType, HarfBuzz, Cairo and Pango runtime as a user."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, tarfile, urllib.request
ROOT = pathlib.Path(__file__).resolve().parents[1]
NATIVE = ROOT / 'native/bitmap-runtime'

def digest(path):
    with path.open('rb') as stream: return hashlib.file_digest(stream, 'sha256').hexdigest()

def run(command, **kwargs):
    subprocess.run([str(s) for s in command], check=True, **kwargs)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=pathlib.Path, default=ROOT / 'build/bitmap-runtime')
    parser.add_argument('--work', type=pathlib.Path, default=ROOT / 'build/native-bitmap')
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('--buildtype', choices=['release', 'debugoptimized', 'debug'], default='release')
    parser.add_argument('--sdk32', type=pathlib.Path, help='User-only i686 SDK prepared from development RPMs')
    parser.add_argument('--portable', action='store_true', help='Omit local CPU tuning')
    args = parser.parse_args()
    if os.geteuid() == 0: parser.error('Build as the regular user; elevate only installation')
    if args.jobs < 1: parser.error('--jobs must be positive')
    prefix, work = args.prefix.resolve(), args.work.resolve()
    if args.sdk32: work = work / 'i686'
    work.mkdir(parents=True, exist_ok=True)
    lock = json.loads((NATIVE / 'sources.json').read_text())
    env = dict(os.environ)
    flags = '-O3 -fPIC -fstack-protector-strong -D_FORTIFY_SOURCE=3 -fstack-clash-protection -fcf-protection=full' + ('' if args.portable else ' -march=native -mtune=native')
    env.setdefault('LDFLAGS', '-Wl,-z,relro,-z,now')
    env.setdefault('CFLAGS', flags); env.setdefault('CXXFLAGS', flags)
    env['PKG_CONFIG_PATH'] = str(prefix / 'lib/pkgconfig') + ':' + env.get('PKG_CONFIG_PATH', '')
    env['LD_LIBRARY_PATH'] = str(prefix / 'lib') + ':' + env.get('LD_LIBRARY_PATH', '')
    cross = []
    if args.sdk32:
        sdk = args.sdk32.resolve()
        bits = sorted((sdk / 'usr/include/c++').glob('*/i686-redhat-linux'))
        if not bits: parser.error('The i686 SDK needs libstdc++-devel headers')
        env['CFLAGS'] = flags + ' -isystem ' + str(sdk / 'usr/include')
        env['CXXFLAGS'] = env['CFLAGS'] + ' -isystem ' + str(bits[-1])
        env['LDFLAGS'] += ' -B' + str(sdk / 'usr/lib') + '/ -L' + str(sdk / 'usr/lib')
        env['PKG_CONFIG_PATH'] = str(prefix / 'lib/pkgconfig') + ':' + str(sdk / 'usr/lib/pkgconfig')
        env['PKG_CONFIG_LIBDIR'] = str(sdk / 'usr/lib/pkgconfig') + ':/usr/share/pkgconfig'
        crossfile = work / 'i686.ini'
        crossfile.write_text("[binaries]\nc = ['gcc', '-m32']\ncpp = ['g++', '-m32']\npkg-config = 'pkg-config'\n[host_machine]\nsystem = 'linux'\ncpu_family = 'x86'\ncpu = 'i686'\nendian = 'little'\n")
        cross = ['--cross-file', str(crossfile)]
    notices = prefix / 'share/domed-bdf-runtime'
    notices.mkdir(parents=True, exist_ok=True)
    (notices / 'manifest.json').unlink(missing_ok=True)
    options = {
        'freetype': ['-Dharfbuzz=disabled', '-Dtests=disabled'],
        'harfbuzz': ['-Dtests=disabled', '-Ddocs=disabled', '-Dintrospection=disabled', '-Dutilities=enabled', '-Dgraphite2=enabled', '-Dchafa=disabled', '-Dgpu_demo=disabled', '-Dcairo=enabled', '-Dicu=enabled', '-Dglib=enabled', '-Dgobject=enabled', '-Dfreetype=enabled'],
        'cairo': ['-Dtests=disabled', '-Dgtk_doc=false', '-Dspectre=disabled'],
        'pango': ['-Dbuild-testsuite=false', '-Dbuild-examples=false', '-Dintrospection=disabled'],
    }
    if args.sdk32:
        # These companion packages are not installed for i686 on DomeD; the
        # core, GObject and subset ABIs are retained for both architectures.
        options['harfbuzz'] = [s for s in options['harfbuzz'] if not s.startswith(('-Dcairo=', '-Dicu=', '-Dutilities='))] + ['-Dcairo=disabled', '-Dicu=disabled', '-Dutilities=disabled']
    for item in lock['sources']:
        name = item['name']; patch = NATIVE / item['patch']
        if digest(patch) != item['patch_sha256']: raise SystemExit('Patch checksum mismatch: ' + name)
        archive = work / item['file']
        if not archive.exists():
            with urllib.request.urlopen(item['url'], timeout=60) as response, archive.open('wb') as output:
                shutil.copyfileobj(response, output)
        if digest(archive) != item['sha256']: raise SystemExit('Archive checksum mismatch: ' + name)
        source = work / (name + '-' + item['patch_sha256'][:16])
        if not source.exists():
            unpack = work / ('unpack-' + name); unpack.mkdir(exist_ok=True)
            with tarfile.open(archive) as tar: tar.extractall(unpack, filter='data')
            extracted = unpack / item['directory']; extracted.rename(source)
            run(['patch', '-p1', '--input', patch], cwd=source)
        run(['patch', '--dry-run', '-R', '-p1', '--input', patch], cwd=source)
        build = work / (name + '-build-' + item['patch_sha256'][:16])
        run(['meson', 'setup', build, source, '--prefix=' + str(prefix), '--libdir=lib', '--buildtype=' + args.buildtype, '--wrap-mode=nofallback',
             *cross, *options[name], *(['--reconfigure'] if (build / 'build.ninja').exists() else [])], env=env)
        run(['ninja', '-C', build, '-j' + str(args.jobs), 'install'], env=env)
        shutil.copy2(archive, notices / archive.name)
        shutil.copy2(patch, notices / patch.name)
        for filename in item['licenses']:
            shutil.copy2(source / filename, notices / (name + '-' + pathlib.Path(filename).name))
    shutil.copy2(NATIVE / 'sources.json', notices / 'sources.json')
    libraries = {p.name: digest(p) for p in (prefix / 'lib').glob('*.so.*') if p.is_symlink()}
    manifest = dict(policy='native-unicode-bitmap-only', formats=['BDF', 'PCF'], buildtype=args.buildtype,
                    local_cpu=not args.portable, arch='i686' if args.sdk32 else 'native', sources=lock['sources'], libraries=libraries)
    (notices / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    if args.sdk32:
        probe = work / 'bitmap-runtime-probe'
        run(['gcc', '-m32', ROOT / 'native/bitmap-runtime/probe.c', '-o', probe,
             '-isystem', str(sdk / 'usr/include'), '-B' + str(sdk / 'usr/lib') + '/', '-L' + str(sdk / 'usr/lib'), '-ldl'], env=env)
        run([probe], env=env)
    else:
        run(['python3', ROOT / 'scripts/check-bitmap-runtime.py', '--manifest', notices / 'manifest.json'], env=env)
    print('Built and verified native BDF/PCF runtime:', prefix)

if __name__ == '__main__': main()
