#!/usr/bin/env python3
"""Build the private, pinned DomeD BDF-only HarfBuzz/Pango fork."""
import argparse, hashlib, json, os, pathlib, shutil, subprocess, tarfile, urllib.request
ROOT = pathlib.Path(__file__).resolve().parents[1]
FORK = ROOT / 'native/harfbuzz-bdf'
LOCK = json.loads((FORK / 'fork.json').read_text())
HB_REVISION = LOCK['fork_revision']
PANGO_SHA256 = '890640c841dae77d3ae3d8fe8953784b930fa241b17423e6120c7bfdf8b891e7'

def run(command, **kwargs):
    subprocess.run([str(s) for s in command], check=True, **kwargs)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prefix', type=pathlib.Path, default=ROOT / 'build/bitmap-runtime')
    parser.add_argument('--buildtype', choices=['debug', 'debugoptimized', 'release'], default='release')
    parser.add_argument('--jobs', type=int, default=2)
    args = parser.parse_args()
    if args.jobs < 1: parser.error('--jobs must be positive')
    prefix = args.prefix.expanduser().resolve()
    work = ROOT / 'build/harfbuzz-bdf'
    work.mkdir(parents=True, exist_ok=True)
    source, build = work / 'source', work / 'build'
    patch = FORK / 'harfbuzz-12.3.2-bdf-only.patch'
    if not source.exists():
        run(['git', 'clone', '--depth', '1', '--branch', LOCK['branch'],
             LOCK['fork'], source])
    revision = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != HB_REVISION: raise SystemExit('Unexpected HarfBuzz source revision')
    if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain']).strip():
        raise SystemExit('Fork checkout has local changes; refusing to overwrite them')
    if digest(patch) != LOCK['patch_sha256']:
        raise SystemExit('HarfBuzz patch differs from the fork lock')
    marker = prefix / 'share/domed-bdf-runtime/manifest.json'
    marker.unlink(missing_ok=True)
    options = ['--prefix=' + str(prefix), '--libdir=lib', '--buildtype=' + args.buildtype,
               '-Dfreetype=enabled', '-Dglib=enabled', '-Dgobject=disabled', '-Dcairo=disabled',
               '-Dicu=disabled', '-Dtests=disabled', '-Dutilities=disabled', '-Ddocs=disabled',
               '-Dintrospection=disabled']
    run(['meson', 'setup', build, source, *options,
         *(['--reconfigure'] if (build / 'build.ninja').exists() else [])])
    run(['ninja', '-C', build, '-j' + str(args.jobs)])
    run(['meson', 'install', '-C', build])
    archive = work / 'pango-1.57.0.tar.xz'
    if not archive.exists():
        installed = pathlib.Path('/opt/fltk-vr/toolkit/share/pango-bdf-source') / archive.name
        if installed.exists(): shutil.copy2(installed, archive)
        else:
            with urllib.request.urlopen('https://download.gnome.org/sources/pango/1.57/' + archive.name, timeout=60) as response:
                archive.write_bytes(response.read())
    if digest(archive) != PANGO_SHA256: raise SystemExit('Pango source checksum mismatch')
    ppatch = FORK / 'pango-1.57.0-bitmap-only.patch'
    psource = work / ('pango-' + digest(ppatch)[:12])
    if not psource.exists():
        unpack = work / 'unpack'
        unpack.mkdir(exist_ok=True)
        with tarfile.open(archive) as tar: tar.extractall(unpack, filter='data')
        (unpack / 'pango-1.57.0').rename(psource)
        run(['patch', '-p1', '--input', ppatch], cwd=psource)
    run(['patch', '--dry-run', '-R', '-p1', '--input', ppatch], cwd=psource)
    pbuild = work / ('pango-build-' + digest(ppatch)[:12])
    env = dict(os.environ, PKG_CONFIG_PATH=str(prefix / 'lib/pkgconfig') + ':' + os.environ.get('PKG_CONFIG_PATH', ''))
    hb_dir = subprocess.check_output(['pkg-config', '--variable=libdir', 'harfbuzz'], env=env, text=True).strip()
    if pathlib.Path(hb_dir).resolve() != prefix / 'lib': raise SystemExit('Pango would link a different HarfBuzz')
    run(['meson', 'setup', pbuild, psource, '--prefix=' + str(prefix), '--libdir=lib',
         '--buildtype=' + args.buildtype, '-Dbuild-testsuite=false', '-Dbuild-examples=false',
         '-Dintrospection=disabled', '-Dlibthai=disabled', '-Dxft=disabled',
         *(['--reconfigure'] if (pbuild / 'build.ninja').exists() else [])], env=env)
    run(['ninja', '-C', pbuild, '-j' + str(args.jobs)], env=env)
    run(['meson', 'install', '-C', pbuild], env=env)
    notices = prefix / 'share/domed-bdf-runtime'
    notices.mkdir(parents=True, exist_ok=True)
    for file in [patch, ppatch, archive, source / 'COPYING', FORK / 'README.md']:
        shutil.copy2(file, notices / file.name)
    shutil.copy2(psource / 'COPYING', notices / 'COPYING.pango')
    run(['git', '-C', source, 'archive', '--format=tar.gz', '--output=' + str(notices / 'harfbuzz-12.3.2-source.tar.gz'), 'HEAD'])
    manifest = dict(harfbuzz_revision=revision, harfbuzz_upstream_revision=LOCK['upstream_revision'], harfbuzz_fork=LOCK['fork'], harfbuzz_patch_sha256=digest(patch),
                    pango_source_sha256=digest(archive), pango_patch_sha256=digest(ppatch),
                    policy='native-unicode-bdf-only', buildtype=args.buildtype,
                    libraries={name: digest(prefix / 'lib' / name) for name in
                               ['libharfbuzz.so.0', 'libpango-1.0.so.0',
                                'libpangoft2-1.0.so.0', 'libpangocairo-1.0.so.0']})
    # The session launcher accepts only a complete build carrying this marker.
    (notices / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print('Built private BDF-only runtime:', prefix)

if __name__ == '__main__': main()
