#!/usr/bin/env python3
"""Exercise session-target dependencies using disposable units on the user manager."""
import os
import json
from pathlib import Path
import subprocess
import tempfile
import time

root = Path(__file__).resolve().parents[1]
units = Path(os.environ['XDG_RUNTIME_DIR']) / 'systemd/user'
units.mkdir(parents=True, exist_ok=True)
base = f'dome-session-test-base-{os.getpid()}.target'
owner = f'dome-session-test-owner-{os.getpid()}.target'
startup = f'dome-session-test-startup-{os.getpid()}.target'
generated_units = []


def systemctl(*args, **kwargs):
    return subprocess.run(['systemctl', '--user', *args], **kwargs)


try:
    (units / base).write_text('[Unit]\nRefuseManualStart=yes\nStopWhenUnneeded=yes\n')
    (units / startup).write_text('[Unit]\nRefuseManualStart=yes\nStopWhenUnneeded=yes\n')
    (units / owner).write_text((root / 'packaging/dome-session.target').read_text()
                              .replace('graphical-session.target', base)
                              .replace('xdg-desktop-autostart.target', startup))
    with tempfile.TemporaryDirectory(prefix='dome-autostart-test-') as folder:
        fixture = Path(folder)
        config = fixture / 'config/autostart'
        config.mkdir(parents=True)
        (fixture / 'system').mkdir()
        marker = fixture / 'started.json'
        worker = fixture / 'worker.py'
        worker.write_text('import json,sys,time\nfrom pathlib import Path\n'
                          'Path(sys.argv[1]).write_text(json.dumps(sys.argv[2:]))\n'
                          'time.sleep(60)\n')
        entry = (f'[Desktop Entry]\nType=Application\nName=Login fixture\n'
                 f'Exec=/usr/bin/python3 {worker} {marker} --tray\n'
                 'TryExec=/usr/bin/python3\nHidden=false\n'
                 'X-GNOME-Autostart-enabled=true\n')
        name = f'org.domed.SessionFixture{os.getpid()}'
        (config / f'{name}.desktop').write_text(entry)
        (config / f'{name}Hidden.desktop').write_text(entry.replace('Hidden=false', 'Hidden=true'))
        # Penguin Mail disables login with the standard Hidden key as well as
        # the GNOME preference; the latter alone does not apply in DomeD.
        (config / f'{name}Disabled.desktop').write_text(
            entry.replace('enabled=true', 'enabled=false').replace('Hidden=false', 'Hidden=true'))
        output = fixture / 'generated'
        output.mkdir()
        env = {**os.environ, 'HOME':str(fixture), 'XDG_CONFIG_HOME':str(config.parent),
               'XDG_CONFIG_DIRS':str(fixture / 'system'), 'XDG_CURRENT_DESKTOP':'Dome'}
        subprocess.run(['/usr/lib/systemd/user-generators/systemd-xdg-autostart-generator',
                        str(output), str(output), str(output)], env=env, check=True)
        services = list(output.glob('*.service'))
        assert len(services) == 1, 'Disabled and hidden entries must not start'
        for service in services:
            generated_units.append(service.name)
            (units / service.name).write_text(service.read_text()
                .replace('graphical-session.target', base)
                .replace('xdg-desktop-autostart.target', startup))
        wants = units / f'{startup}.wants'
        wants.mkdir()
        for service in services:
            (wants / service.name).symlink_to(units / service.name)
        systemctl('daemon-reload', check=True)
        assert systemctl('start', base, stdout=subprocess.DEVNULL,
                         stderr=subprocess.DEVNULL).returncode != 0
        systemctl('start', owner, check=True)
        deadline = time.monotonic() + 5
        while not marker.exists():
            assert time.monotonic() < deadline, 'XDG login application did not start'
            time.sleep(.05)
        assert json.loads(marker.read_text()) == ['--tray']
        assert systemctl('is-active', '--quiet', base).returncode == 0
        assert systemctl('is-active', '--quiet', startup).returncode == 0
        assert systemctl('is-active', '--quiet', generated_units[0]).returncode == 0
        systemctl('stop', owner, check=True)
        deadline = time.monotonic() + 5
        while any(systemctl('is-active', '--quiet', unit).returncode == 0
                  for unit in [base, startup, *generated_units]):
            assert time.monotonic() < deadline, 'Login applications outlived their session owner'
            time.sleep(.05)
        print('PASS: session starts generated XDG application with --tray, skips disabled/hidden entries, and releases applications and targets on logout')
finally:
    systemctl('stop', owner, startup, *generated_units, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wants = units / f'{startup}.wants'
    if wants.exists():
        for link in wants.iterdir():
            link.unlink()
        wants.rmdir()
    for unit in [owner, base, startup, *generated_units]:
        (units / unit).unlink(missing_ok=True)
    systemctl('daemon-reload', check=True)
