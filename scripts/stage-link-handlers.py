#!/usr/bin/python3
"""Stage working-mode routing for installed de-xr application launchers."""
import argparse
import json
from pathlib import Path
import shutil
import hashlib
import os
import tempfile
import datetime

ROOT = Path(__file__).resolve().parents[1]


def prefix(options):
    return '/usr/local/bin/dome-link' + (' --realm ' + options['realm'] if options.get('realm') else '') + (' --app-id ' + options['app_id'] if options.get('app_id') else '') + ' -- '


def desktop(text, options):
    main = False
    output = []
    for line in text.splitlines():
        if line.startswith('['):
            main = line == '[Desktop Entry]'
        if line.startswith('Exec='):
            command = line[5:]
            if options.get('automatic'):
                command = command.replace(' --domed ', ' ')
            explicit = any(flag in command for flag in ['--xr-output=', '-dome-xr', 'gmsh-dome-xr', '--openxr', '--xr '])
            if 'dome-link ' not in command and not explicit:
                selected = dict(options) if options.get('reuse') else {}
                if '--desktop' in command: selected['realm'] = 'desktop'
                line = 'Exec=' + prefix(selected) + command
        if main and options.get('automatic') and line.startswith('Comment='):
            line = 'Comment=Play media in the current DomeD working mode'
        if main and line == 'DBusActivatable=true':
            # Our executable routes first; its own instance protocol still reuses
            # the application after the window has moved to the requested realm.
            line = 'DBusActivatable=false'
        output.append(line)
    return '\n'.join(output) + '\n'


def wrapper(text, options):
    lines = text.splitlines()
    if 'dome-link' in text:
        return text
    if not lines or not lines[0].startswith('#!/bin/sh') or not lines[-1].startswith('exec '):
        raise ValueError('Expected an installed POSIX shell launcher')
    original = lines[-1]
    lines[-1:] = ['case "${1-}" in', '  --help|--version|--xr-probe|--dome-present|--sbs-present|--model-xr|--openxr|--xr) ' + original + ' ;;', 'esac',
                  'exec ' + prefix(options) + original[5:]]
    return '\n'.join(lines) + '\n'


def stage(destination, installed=Path('/')):
    config = json.loads((ROOT / 'packaging/link-handlers.json').read_text())
    paths = []
    user_paths = []

    def write(name, text, executable=False):
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)
        target.chmod(0o755 if executable else 0o644)
        paths.append(name)

    write('usr/local/bin/dome-link', (ROOT / 'packaging/dome-link').read_text(), True)
    for name, options in config['applications'].items():
        path = 'usr/local/share/applications/' + name
        source = installed / path
        if source.is_file():
            write(path, desktop(source.read_text(), options))
    for name, options in config['wrappers'].items():
        path = 'usr/local/bin/' + name
        source = installed / path
        if source.is_file():
            write(path, wrapper(source.read_text(), options), True)
    user_dir = Path(os.environ.get('XDG_DATA_HOME', str(Path.home()/'.local/share'))) / 'applications'
    for name, options in config['applications'].items():
        source = user_dir / name
        if source.is_file() and source.stat().st_uid == os.getuid():
            target = destination / 'user-applications' / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(desktop(source.read_text(), options))
            user_paths.append({'source':str(source),'staged':str(target.resolve()),
                'before':hashlib.sha256(source.read_bytes()).hexdigest(),
                'after':hashlib.sha256(target.read_bytes()).hexdigest()})
    (destination/'user-link-handler-files.json').write_text(json.dumps(user_paths,indent=2)+'\n')
    audit = ROOT/'docs/WORKING_MODE_AUDIT.md'
    if audit.is_file():
        write('usr/local/share/doc/domed/docs/WORKING_MODE_AUDIT.md', audit.read_text())
    write('usr/local/share/domed/link-handlers.json', json.dumps(config, indent=2) + '\n')
    (destination / 'link-handler-files.json').write_text(json.dumps(paths, indent=2) + '\n')
    return paths


def apply_user(destination):
    paths = json.loads((destination/'user-link-handler-files.json').read_text())
    backup = Path(os.environ.get('XDG_STATE_HOME',str(Path.home()/'.local/state')))/'dome/backups'/('link-handlers-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S'))
    for row in paths:
        source, staged = Path(row['source']), Path(row['staged'])
        assert source.stat().st_uid == os.getuid()
        assert hashlib.sha256(source.read_bytes()).hexdigest() == row['before'], str(source)
        assert hashlib.sha256(staged.read_bytes()).hexdigest() == row['after'], str(staged)
    for row in paths:
        source, staged = Path(row['source']), Path(row['staged'])
        backup.mkdir(parents=True,exist_ok=True);shutil.copy2(source,backup/source.name)
        fd, temporary = tempfile.mkstemp(prefix='.dome-link-',dir=source.parent)
        with os.fdopen(fd,'wb') as output:output.write(staged.read_bytes());os.fchmod(output.fileno(),source.stat().st_mode & 0o777)
        os.replace(temporary,source)
    return str(backup)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('stage', type=Path)
    parser.add_argument('--apply-user', action='store_true', help='Apply already staged user-owned desktop entries')
    parser.add_argument('--installed-root', type=Path, default=Path('/'))
    args = parser.parse_args()
    print(apply_user(args.stage) if args.apply_user else json.dumps(stage(args.stage, args.installed_root), indent=2))
