#!/usr/bin/env python3
"""Check login preparation without starting or stopping any real session."""
import os
from pathlib import Path
import runpy
import subprocess
from unittest.mock import patch

script = Path(__file__).resolve().parents[1] / 'packaging/dome-session-client'


def check(login, reload_status=0):
    calls = []

    class Child:
        pid = 2147483647

        def wait(self):
            return 0

    def run(args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, reload_status)

    def spawn(args, **kwargs):
        calls.append(args)
        return Child()

    with patch.dict(os.environ, {'DOME_LOGIN_SESSION': login, 'DOME_SESSION_TERMINAL': '0'}), \
            patch('subprocess.run', run), patch('subprocess.Popen', spawn), \
            patch('os.killpg', side_effect=ProcessLookupError), patch('signal.signal'):
        try:
            runpy.run_path(str(script), run_name='__main__')
        except SystemExit as result:
            assert result.code == 0
    expected = [['/usr/local/bin/dome-compositor', '--backend', 'drm'],
                ['systemctl', '--user', 'stop', 'dome-session.target']]
    if login == '1':
        expected.insert(0, ['systemctl', '--user', 'daemon-reload'])
    assert calls == expected, calls


check('1')
check('1', reload_status=1)
check('0')
print('PASS: real login refreshes generated entries before compositor launch; reload failure preserves startup; other invocations skip refresh; logout cleanup retained')
