#!/usr/bin/env python3
"""Exercise login recovery without starting a compositor, bus, systemd or seat."""
import os
from pathlib import Path
import runpy
import signal
import tempfile
import unittest
from unittest.mock import Mock, patch
ROOT=Path(__file__).resolve().parents[1]
class Recovery(unittest.TestCase):
    def scenario(self, exits, explicit=False, active=True, during_backoff=False):
        with tempfile.TemporaryDirectory() as directory:
            marker=Path(directory)/'dome/explicit-stop';marker.parent.mkdir()
            children=[];handlers={}
            def spawn(*args,**kwargs):
                child=Mock(pid=900000+len(children))
                code=exits[len(children)]
                def wait():
                    if explicit or len(children)==len(exits):marker.touch()
                    return code
                child.wait.side_effect=wait;children.append(child);return child
            def sleep(_):
                if during_backoff:handlers[signal.SIGTERM](signal.SIGTERM,None)
            with patch.dict(os.environ,{'XDG_RUNTIME_DIR':directory,'DOME_LOGIN_SESSION':'0'}), patch('signal.signal',side_effect=lambda sig,fn:handlers.__setitem__(sig,fn)), patch('subprocess.Popen',side_effect=spawn), patch('subprocess.run',return_value=Mock(returncode=0 if active else 1)) as control, patch('os.killpg',side_effect=ProcessLookupError), patch('time.sleep',side_effect=sleep):
                with self.assertRaises(SystemExit):runpy.run_path(str(ROOT/'packaging/dome-session-client'),run_name='__main__')
                self.assertIn(unittest.mock.call(['systemctl','--user','stop','dome-session.target'],check=False),control.call_args_list)
            return len(children)
    def test_crash_restarts_in_same_login(self):self.assertEqual(self.scenario([-9,0]),2)
    def test_explicit_stop_never_restarts(self):self.assertEqual(self.scenario([-15],explicit=True),1)
    def test_inactive_wivrn_does_not_supervise_login(self):self.assertEqual(self.scenario([-9,0],active=False),1)
    def test_termination_during_backoff_does_not_start_another_child(self):self.assertEqual(self.scenario([-9,0],during_backoff=True),1)
if __name__=='__main__':unittest.main()
