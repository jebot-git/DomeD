#!/usr/bin/env python3
"""Verify all merged glyph pixels/metrics against pinned Unicode-selected sources."""
import hashlib
import importlib.util
import json
from pathlib import Path
import zipfile

spec = importlib.util.spec_from_file_location('merge', Path(__file__).with_name('merge-script-fonts.py'))
merge = importlib.util.module_from_spec(spec); spec.loader.exec_module(merge)
assert [merge.source(ord(c)) for c in 'Aあア가ㄅ漢'] == ['latin','ja','ja','ko','zh_tw','zh_hans']
assert merge.source(0x31350) == 'zh_hans' # CJK extension H, beyond BMP
checks = 0
for name in ['fusion','ark','jelly']:
    d = merge.ASSETS / f'TakWolf--{name}-pixel-font'
    manifest = json.loads((d/'upstream.json').read_text())
    groups = {}
    with zipfile.ZipFile(d/'upstream-scripts.zip') as archive:
        for n, digest in manifest['source_files'].items():
            data = archive.read(n); assert hashlib.sha256(data).hexdigest() == digest
            stem, lang = Path(n).stem.rsplit('-',1)
            groups.setdefault(stem,{})[lang] = merge.parse(data.decode())[1]
    output = json.loads((d/'mixed.json').read_text())['files']
    for stem, variants in groups.items():
        path = d/(stem+'-mixed.bdf')
        assert hashlib.sha256(path.read_bytes()).hexdigest() == output[path.name]
        _, merged = merge.parse(path.read_text())
        assert set(merged) == set().union(*(set(g) for g in variants.values()))
        for cp, glyph in merged.items():
            language = next(lang for lang in [merge.source(cp)]+merge.ORDER if lang in variants and cp in variants[lang])
            assert glyph == variants[language][cp], (path, cp, language)
            checks += 1
print(f'PASS: {checks} mixed-script glyphs retain exact source pixels, advances and bearings; Unicode 17 script selection and union coverage verified')
