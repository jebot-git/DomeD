#!/usr/bin/env python3
"""Verify that distribution cleanup preserves locally modified and personal fonts."""
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('retired', Path(__file__).with_name('prune-retired-fonts.py'))
retired = importlib.util.module_from_spec(spec)
spec.loader.exec_module(retired)


class RetirementTests(unittest.TestCase):
    def test_backups_and_owned_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prefix = root / 'system'
            fonts = prefix / 'usr/local/share/fonts/domed'
            fonts.mkdir(parents=True)
            for name, data in [('retired.bdf', 'original'), ('modified.bdf', 'personal'), ('personal.bdf', 'custom')]:
                (fonts / name).write_text(data)
            outside = root / 'outside'
            outside.mkdir()
            (outside / 'linked.bdf').write_text('original')
            (fonts / 'link').symlink_to(outside, target_is_directory=True)
            digest = hashlib.sha256(b'original').hexdigest()
            manifest = root / 'manifest.json'
            manifest.write_text(json.dumps({name: digest for name in ['retired.bdf', 'modified.bdf', 'link/linked.bdf']}))
            self.assertEqual(retired.prune(manifest, root / 'backup', prefix), 1)
            self.assertFalse((fonts / 'retired.bdf').exists())
            self.assertEqual((fonts / 'modified.bdf').read_text(), 'personal')
            self.assertEqual((fonts / 'personal.bdf').read_text(), 'custom')
            self.assertEqual((outside / 'linked.bdf').read_text(), 'original')
            self.assertEqual((root / 'backup/usr/local/share/fonts/domed/retired.bdf').read_text(), 'original')


if __name__ == '__main__':
    unittest.main()
