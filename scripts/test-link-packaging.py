#!/usr/bin/python3
"""Check launcher routing, field-code preservation and explicit-mode exceptions."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('stage_links',ROOT/'scripts/stage-link-handlers.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class Packaging(unittest.TestCase):
    def test_uri_fields_and_actions_keep_literal_arguments_and_explicit_output(self):
        source='[Desktop Entry]\nType=Application\nName=Fixture\nDBusActivatable=true\nExec=browser "two words" %u\nActions=Calendar;Desktop;XR;\n[Desktop Action Calendar]\nName=Calendar\nExec=browser --calendar-today\n[Desktop Action Desktop]\nName=Desktop\nExec=fsvr --desktop %U\n[Desktop Action XR]\nName=XR\nExec=gmsh-dome-xr %F\n'
        result=module.desktop(source,{'reuse':True,'app_id':'org.domed.Fixture'})
        self.assertIn('Exec=/usr/local/bin/dome-link --app-id org.domed.Fixture -- browser "two words" %u',result)
        self.assertIn('-- browser --calendar-today',result)
        self.assertIn('--realm desktop --app-id org.domed.Fixture -- fsvr --desktop %U',result)
        self.assertIn('Exec=gmsh-dome-xr %F',result)
        self.assertIn('DBusActivatable=false',result)
        self.assertEqual(module.desktop(result,{'reuse':True,'app_id':'org.domed.Fixture'}),result)
    def test_mime_video_default_does_not_force_xr(self):
        result=module.desktop('[Desktop Entry]\nExec=mpvr --domed %f\n',{'automatic':True})
        self.assertEqual(result,'[Desktop Entry]\nExec=/usr/local/bin/dome-link -- mpvr %f\n')
    def test_wrapper_preserves_environment_and_internal_presenter_options(self):
        source='#!/bin/sh\nexport TEST=value\nexec "$prefix/bin/browser" "$@"\n'
        result=module.wrapper(source,{'app_id':'org.domed.Browser'})
        self.assertIn('export TEST=value',result)
        self.assertIn('--dome-present|--sbs-present',result)
        self.assertIn('exec /usr/local/bin/dome-link --app-id org.domed.Browser -- "$prefix/bin/browser" "$@"',result)
        self.assertEqual(module.wrapper(result,{}),result)
        with tempfile.NamedTemporaryFile(mode='w')as script:
            script.write(result);script.flush();subprocess.run(['sh','-n',script.name],check=True)
if __name__=='__main__':unittest.main()
