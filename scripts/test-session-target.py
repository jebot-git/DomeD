#!/usr/bin/env python3
"""Exercise session-target dependencies using disposable units on the user manager."""
import os
from pathlib import Path
import subprocess
import time

root = Path(__file__).resolve().parents[1]
units = Path(os.environ['XDG_RUNTIME_DIR']) / 'systemd/user'
units.mkdir(parents=True, exist_ok=True)
base = f'dome-session-test-base-{os.getpid()}.target'
owner = f'dome-session-test-owner-{os.getpid()}.target'


def systemctl(*args, **kwargs):
    return subprocess.run(['systemctl', '--user', *args], **kwargs)


try:
    (units / base).write_text('[Unit]\nRefuseManualStart=yes\nStopWhenUnneeded=yes\n')
    (units / owner).write_text((root / 'packaging/dome-session.target').read_text()
                              .replace('graphical-session.target', base))
    systemctl('daemon-reload', check=True)
    assert systemctl('start', base, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL).returncode != 0
    systemctl('start', owner, check=True)
    assert systemctl('is-active', '--quiet', base).returncode == 0
    systemctl('stop', owner, check=True)
    deadline = time.monotonic() + 5
    while systemctl('is-active', '--quiet', base).returncode == 0:
        assert time.monotonic() < deadline, 'Base target outlived its session owner'
        time.sleep(.05)
    print('PASS: session dependency starts restricted graphical target and releases it on logout')
finally:
    systemctl('stop', owner, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    (units / owner).unlink(missing_ok=True)
    (units / base).unlink(missing_ok=True)
    systemctl('daemon-reload', check=True)
