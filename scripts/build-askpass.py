#!/usr/bin/env python3
"""Build pinned GnuPG Pinentry with DomeD's FLTK frontend and sudo mode."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
VERSION = '1.3.3'
SHA256 = 'c2970f16d6afb66ecddfca767d743936c86239bff936eed7fd7597a678414b63'
URL = f'https://www.gnupg.org/ftp/gcrypt/pinentry/pinentry-{VERSION}.tar.bz2'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fltk-prefix', type=Path, default=Path(os.environ.get('DOME_FLTK_PREFIX', '/opt/fltk-vr/toolkit')))
    parser.add_argument('--deps', type=Path, default=ROOT / '.local-deps/pinentry')
    parser.add_argument('--build', type=Path, default=ROOT / 'build/askpass')
    args = parser.parse_args()
    prefix, deps, build = args.fltk_prefix.resolve(), args.deps.resolve(), args.build.resolve()
    config = prefix / 'bin/fltk-config'
    if subprocess.check_output([config, '--version'], text=True).strip() != '1.4.5':
        raise SystemExit('Use the shared patched FLTK 1.4.5 toolkit')
    deps.mkdir(parents=True, exist_ok=True)
    archive = deps / f'pinentry-{VERSION}.tar.bz2'
    if not archive.exists():
        urllib.request.urlretrieve(URL, archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise SystemExit('Pinentry archive checksum mismatch')
    patch = ROOT / 'native/pinentry/pinentry-1.3.3-dome.patch'
    digest = hashlib.sha256(patch.read_bytes()).hexdigest()
    source = deps / f'pinentry-{VERSION}-dome'
    stamp = source / '.dome-patch'
    if not stamp.exists() or stamp.read_text().strip() != digest:
        if source.exists():
            shutil.rmtree(source)  # This directory contains only generated, pinned sources.
        with tarfile.open(archive) as stream:
            # Extract into a temporary directory so the unmodified archive stays available.
            import tempfile
            with tempfile.TemporaryDirectory(dir=deps) as temporary:
                stream.extractall(temporary, filter='data')
                shutil.move(str(Path(temporary) / f'pinentry-{VERSION}'), source)
        subprocess.run(['patch', '--batch', '-p1', '-i', patch], cwd=source, check=True)
        stamp.write_text(digest + '\n')
    build.mkdir(parents=True, exist_ok=True)
    libraries = (prefix / 'lib').resolve()
    env = {**os.environ, 'FLTK_CONFIG': str(config)}
    cflags = '-O3 -DNDEBUG -flto'
    includes = subprocess.check_output(['pkg-config', '--cflags', 'fontconfig'], text=True).strip()
    env.update(CFLAGS=cflags, CXXFLAGS=f'{cflags} -std=c++17 {includes} -I{ROOT / "native"} -I{ROOT / "native/pinentry"}',
               LDFLAGS=f'-flto -Wl,-rpath,{libraries}',
               LIBS=subprocess.check_output(['pkg-config', '--libs', 'fontconfig', 'zlib'], text=True).strip())
    configure = [source / 'configure', '--prefix=/usr/local', '--enable-pinentry-fltk',
                 '--disable-fallback-curses', '--disable-pinentry-curses', '--disable-pinentry-tty',
                 '--disable-pinentry-emacs', '--disable-inside-emacs', '--disable-pinentry-efl',
                 '--disable-pinentry-gtk2', '--disable-pinentry-gnome3', '--disable-pinentry-qt',
                 '--disable-pinentry-qt5', '--disable-pinentry-qt4', '--disable-pinentry-tqt', '--disable-libsecret']
    subprocess.run(configure, cwd=build, env=env, check=True)
    subprocess.run(['make', '-j' + os.environ.get('DOME_BUILD_JOBS', '8')], cwd=build, env=env, check=True)
    launcher = (ROOT / 'packaging/dome-askpass.in').read_text().replace('@FLTK_LIBDIR@', str(libraries))
    (build / 'dome-askpass').write_text(launcher)
    (build / 'dome-askpass').chmod(0o755)
    shutil.copy2(source / 'COPYING', build / 'COPYING.pinentry')
    shutil.copy2(archive, build / archive.name)
    (build / 'source.sha256').write_text(SHA256 + '  ' + archive.name + '\n')
    print('Built:', build / 'fltk/pinentry-fltk')


if __name__ == '__main__':
    main()
