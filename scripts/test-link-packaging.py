#!/usr/bin/python3
"""Check launcher routing, field-code preservation and explicit-mode exceptions."""
import importlib.util
import json
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
    def test_torrent_handler_routes_reused_window_and_preserves_mime_fields(self):
        options=json.loads((ROOT/'packaging/link-handlers.json').read_text())['applications']['org.domed.Torrent.desktop']
        source='[Desktop Entry]\nType=Application\nName=Dome Torrent\nExec=dome-torrent %U\nTryExec=dome-torrent\nMimeType=application/x-bittorrent;x-scheme-handler/magnet;\n'
        result=module.desktop(source,options)
        self.assertIn('Exec=/usr/local/bin/dome-link --app-id org.domed.Torrent -- dome-torrent %U',result)
        self.assertIn('MimeType=application/x-bittorrent;x-scheme-handler/magnet;',result)
        self.assertIn('TryExec=dome-torrent',result)
        self.assertEqual(module.desktop(result,options),result)
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
    def test_browser_menu_activation_preserves_caller_and_uri_dispatch_stays_automatic(self):
        options=json.loads((ROOT/'packaging/link-handlers.json').read_text())['wrappers']['dome-browser']
        source='#!/bin/sh\nexport TEST=value\nexec browser "$@"\n'
        routed=module.wrapper(source,{'app_id':'org.domed.Browser'})
        result=module.wrapper(routed,options)
        self.assertEqual(module.wrapper(source,options),result)
        self.assertEqual(module.wrapper(result,options),result)
        with tempfile.TemporaryDirectory() as folder:
            folder=Path(folder);recorder=folder/'record';script=folder/'browser'
            recorder.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n');recorder.chmod(0o755)
            script.write_text(result.replace('/usr/local/bin/dome-link',str(recorder)))
            no_args=subprocess.check_output(['sh',str(script)],text=True).splitlines()
            self.assertEqual(no_args,['--realm','caller','--app-id','org.domed.Browser','--','browser'])
            uri='https://example.invalid/a b?x=$(false)&v=ü#fragment'
            with_uri=subprocess.check_output(['sh',str(script),uri],text=True).splitlines()
            self.assertEqual(with_uri,['--app-id','org.domed.Browser','--','browser',uri])
if __name__=='__main__':unittest.main()
