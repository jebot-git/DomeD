#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
import importlib.util
import io
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from unittest.mock import patch
import urllib.error

SOURCE = Path(__file__).resolve().parents[2] / 'native/translator/backend.py'
spec = importlib.util.spec_from_file_location('translator', SOURCE)
b = importlib.util.module_from_spec(spec)
spec.loader.exec_module(b)
CAPS = [{'lang': x, 'usable_as_source': src, 'usable_as_target': dst}
        for x, src, dst in [('en', True, False), ('en-US', False, True),
                            ('cs', True, True), ('sr', True, True),
                            ('ja', True, True), ('ko', True, True)]]


class TranslatorTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.env = patch.dict(os.environ, {'XDG_CONFIG_HOME': self.directory.name,
                                          'DEEPL_API_KEY': ''})
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_profile_current_group_order_and_deduplication(self):
        profile = Path(self.directory.name) / 'dome/fcitx5/profile'
        profile.parent.mkdir(parents=True)
        profile.write_text('[Groups/0]\nName=Other\n[Groups/0/Items/0]\nName=keyboard-de\n'
                           '[Groups/1]\nName=Default\n[Groups/1/Items/0]\nName=keyboard-us\n'
                           '[Groups/1/Items/1]\nName=anthy\n[Groups/1/Items/2]\nName=keyboard-cz\n'
                           '[Groups/1/Items/3]\nName=keyboard-rs\n[Groups/1/Items/4]\nName=hangul\n'
                           '[Groups/1/Items/5]\nName=mozc\n[GroupOrder]\n0=Default\n')
        with patch.object(b, 'dbus', side_effect=ValueError):
            self.assertEqual([x['code'] for x in b.local_languages()], ['en', 'ja', 'cs', 'sr', 'ko'])

    def test_live_fcitx_group_takes_precedence(self):
        with patch.object(b, 'dbus', side_effect=[('Work',), ('cz', [('keyboard-cz', ''), ('anthy', '')])]):
            self.assertEqual([x['code'] for x in b.local_languages()], ['cs', 'ja'])

    def test_private_key_save_environment_precedence_and_remove(self):
        b.save_key('test:fx')
        self.assertEqual(stat.S_IMODE(b.key_path().stat().st_mode), 0o600)
        self.assertEqual(b.api_key(), 'test:fx')
        with patch.dict(os.environ, {'DEEPL_API_KEY': 'environment:fx'}):
            self.assertEqual(b.api_key(), 'environment:fx')
        b.key_path().chmod(0o644)
        with self.assertRaisesRegex(ValueError, 'private'):
            b.api_key()
        b.save_key('')
        self.assertFalse(b.key_path().exists())

    def test_pro_keys_and_header_injection_rejected(self):
        for key in ['pro-key', 'foo\nbar:fx', 'foo\rbar:fx']:
            with self.assertRaises(ValueError):
                b.save_key(key)
            with patch.dict(os.environ, {'DEEPL_API_KEY': key}):
                with self.assertRaises(ValueError):
                    b.api_key()

    def test_source_and_target_capabilities_are_distinct(self):
        self.assertEqual(b.resolve('en', CAPS, False), 'en')
        self.assertEqual(b.resolve('en', CAPS, True), 'en-US')
        self.assertIsNone(b.resolve('xx', CAPS, True))
        self.assertIsNone(b.resolve('', CAPS, False))

    def test_no_key_only_reads_local_languages(self):
        with patch.object(b, 'local_languages', return_value=[{'code': 'sr', 'name': 'Serbian'}]), \
                patch.object(b, 'request') as request:
            result = b.run({'action': 'languages'})
            self.assertFalse(result['api_ready'])
            request.assert_not_called()

    def test_network_failure_keeps_local_language_menus(self):
        b.save_key('test:fx')
        with patch.object(b, 'local_languages', return_value=[{'code': 'sr', 'name': 'Serbian'}]), \
                patch.object(b, 'capabilities', side_effect=ValueError('Cannot reach DeepL.')):
            result = b.run({'action': 'languages'})
            self.assertEqual(result['languages'][0]['code'], 'sr')
            self.assertIn('Cannot reach', result['notice'])

    def test_missing_deepl_language_disabled(self):
        b.save_key('test:fx')
        with patch.object(b, 'local_languages', return_value=[{'code': 'sr', 'name': 'Serbian'}]), \
                patch.object(b, 'capabilities', return_value=CAPS[:2]):
            result = b.run({'action': 'languages'})
            self.assertFalse(result['languages'][0]['target'])
            self.assertFalse(result['languages'][0]['source'])

    def test_no_transcription_for_english_czech_serbian(self):
        with patch.object(b, 'icu_latin', side_effect=AssertionError('must not transliterate')):
            for language, text in [('en', 'Hello'), ('cs', 'Příliš žluťoučký kůň'),
                                   ('sr', 'Здраво, Љиљана!'), ('sr', 'Zdravo, Ljiljana!')]:
                self.assertEqual(b.transcription(text, language), '')

    def test_japanese_kanji_dictionary_and_korean(self):
        self.assertIn('nihongo', b.transcription('日本語', 'ja'))
        self.assertIn('toukyou', b.transcription('東京', 'ja'))
        self.assertEqual(b.transcription('안녕하세요', 'ko'), 'annyeonghaseyo')
        self.assertEqual(b.transcription('한국어', 'ko'), 'hangug-eo')

    def test_other_scripts_and_cyrillic(self):
        self.assertTrue(b.transcription('Καλημέρα', 'el').isascii())
        self.assertEqual(b.transcription('Здравствуйте', 'ru'), '')

    def translate(self, source, target, text, translated, detected='EN'):
        def response(path, body=None):
            self.assertEqual(path, '/v2/translate')
            self.assertEqual(body['text'], [text])
            self.assertEqual(body['target_lang'], b.resolve(target, CAPS, True))
            if source:
                self.assertEqual(body['source_lang'], source)
            else:
                self.assertNotIn('source_lang', body)
            return {'translations': [{'text': translated, 'detected_source_language': detected}]}
        with patch.object(b, 'local_languages', return_value=[{'code': x} for x in ['en', 'cs', 'sr', 'ja', 'ko']]), \
                patch.object(b, 'capabilities', return_value=CAPS), \
                patch.object(b, 'request', side_effect=response) as request:
            result = b.run({'action': 'translate', 'source': source, 'target': target, 'text': text})
            self.assertEqual(request.call_count, 1)  # never replay a billable POST
            return result

    def test_translation_keeps_script_and_adds_separate_reading(self):
        result = self.translate('en', 'ja', 'Japanese', '日本語')
        self.assertEqual(result['text'], '日本語')
        self.assertIn('nihongo', result['reading'])
        self.assertEqual(result['source_reading'], '')

    def test_auto_detected_source_reading(self):
        result = self.translate('', 'cs', '안녕하세요', 'Ahoj', 'KO')
        self.assertEqual(result['source_reading'], 'annyeonghaseyo')
        self.assertEqual(result['reading'], '')

    def test_serbian_is_preserved_exactly(self):
        result = self.translate('en', 'sr', 'Hello', 'Здраво, Љиљана!')
        self.assertEqual(result['text'], 'Здраво, Љиљана!')
        self.assertEqual(result['reading'], '')

    def test_reading_failure_preserves_billable_result(self):
        with patch.object(b, 'transcription', side_effect=RuntimeError('dictionary missing')):
            result = self.translate('en', 'ja', 'Japanese', '日本語')
        self.assertEqual(result['text'], '日本語')
        self.assertIn('preserved', result['notice'])

    def test_empty_oversize_and_unconfigured_language_never_post(self):
        with patch.object(b, 'request') as request, \
                patch.object(b, 'local_languages', return_value=[{'code': 'en'}]):
            for task in [{'text': ''}, {'text': 'a' * (b.MAX_TEXT + 1)},
                         {'text': 'hello', 'target': 'xx'}]:
                with self.assertRaises(ValueError):
                    b.run({'action': 'translate', 'target': 'en', **task})
            request.assert_not_called()

    def test_wayland_clipboard_and_primary_stay_separate_and_local(self):
        with patch.dict(os.environ, {'WAYLAND_DISPLAY': 'isolated-test'}), \
                patch.object(b.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'Ahoj!\n日本語'.encode(), b'')) as run, \
                patch.object(b, 'request') as request:
            self.assertEqual(b.run({'action': 'paste', 'primary': False})['paste'], 'Ahoj!\n日本語')
            self.assertNotIn('--primary', run.call_args.args[0])
            b.run({'action': 'paste', 'primary': True})
            self.assertIn('--primary', run.call_args.args[0])
            request.assert_not_called()

    def test_http_protocol_errors_no_retry_no_redirect_leak(self):
        received = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_GET(self):
                received.append((self.path, self.headers.get('Authorization')))
                if self.path == '/redirect':
                    self.send_response(302)
                    self.send_header('Location', '/must-not-follow')
                    self.end_headers()
                else:
                    self.send_response(456)
                    self.end_headers()
                    self.wfile.write(b'{"message":"not reflected"}')
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with patch.object(b, 'API', f'http://127.0.0.1:{server.server_port}'):
                for path, message in [('/quota', 'quota'), ('/redirect', 'HTTP 302')]:
                    with self.assertRaisesRegex(ValueError, message):
                        b.request(path, key='test:fx')
            self.assertEqual(received, [('/quota', 'DeepL-Auth-Key test:fx'),
                                        ('/redirect', 'DeepL-Auth-Key test:fx')])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == '__main__':
    unittest.main(verbosity=2)
