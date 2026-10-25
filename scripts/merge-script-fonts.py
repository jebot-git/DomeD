#!/usr/bin/env python3
"""Rebuild mixed-script BDFs from checksum-pinned, unmodified upstream strikes.

Unicode Script selects the source glyph in one pass over the code point union.
Han is unified: code points cannot select a regional form. Use zh_hans for Han,
zh_tw for Bopomofo, ja for Kana, ko for Hangul, latin for other scripts; use the
first available glyph in a stable fallback order when that source is incomplete.
No resampling or glyph geometry changes. Sources and licenses remain packaged.
"""
from bisect import bisect_right
import hashlib
import json
from pathlib import Path
import re
import zipfile

ROOT = Path(__file__).resolve().parents[1]
ASSETS = ROOT / 'assets/fonts'

def parse(text):
    head, tail = re.split(r'^CHARS \d+\n', text, maxsplit=1, flags=re.M)
    glyphs = {}
    for glyph in re.findall(r'^STARTCHAR .*?^ENDCHAR\n', tail, re.M | re.S):
        cp = int(re.search(r'^ENCODING (-?\d+)', glyph, re.M)[1])
        if cp >= 0:
            assert cp not in glyphs
            glyphs[cp] = glyph
    return head, glyphs

def scripts():
    ranges = []
    for line in (ASSETS / 'unicode-17.0.0/Scripts.txt').read_text().splitlines():
        data = line.split('#')[0].strip()
        if not data: continue
        interval, script = map(str.strip, data.split(';'))
        bounds = interval.split('..')
        ranges.append((int(bounds[0], 16), int(bounds[-1], 16), script))
    ranges.sort()
    return ranges

RANGES = scripts()
STARTS = [r[0] for r in RANGES]
ORDER = ['latin', 'zh_hans', 'zh_hant', 'zh_tw', 'zh_hk', 'ja', 'ko']

def source(cp):
    i = bisect_right(STARTS, cp) - 1
    script = RANGES[i][2] if i >= 0 and cp <= RANGES[i][1] else 'Unknown'
    return {'Hiragana': 'ja', 'Katakana': 'ja', 'Hangul': 'ko',
            'Bopomofo': 'zh_tw', 'Han': 'zh_hans'}.get(script, 'latin')

def build(directory):
    manifest = json.loads((directory / 'upstream.json').read_text())
    groups = {}
    with zipfile.ZipFile(directory / 'upstream-scripts.zip') as archive:
        for name, digest in manifest['source_files'].items():
            data = archive.read(name)
            assert hashlib.sha256(data).hexdigest() == digest
            stem, language = Path(name).stem.rsplit('-', 1)
            groups.setdefault(stem, {})[language] = parse(data.decode())
    generated = {}
    for stem, variants in sorted(groups.items()):
        header = variants['latin'][0]
        family = re.search(r'^FAMILY_NAME "(.+)"', header, re.M)[1]
        assert family.endswith(' latin')
        header = header.replace(family, family[:-6])
        union = sorted(set().union(*(set(glyphs) for _, glyphs in variants.values())))
        glyphs = []
        for cp in union:
            preferred = source(cp)
            lang = next(lang for lang in [preferred] + ORDER if lang in variants and cp in variants[lang][1])
            glyphs.append(variants[lang][1][cp])
        # Enclose every original glyph, including variants with taller metrics.
        boxes = [tuple(map(int, re.search(r'^BBX (.*)$', g, re.M)[1].split())) for g in glyphs]
        left = min(x for w,h,x,y in boxes); bottom = min(y for w,h,x,y in boxes)
        right = max(x+w for w,h,x,y in boxes); top = max(y+h for w,h,x,y in boxes)
        header = re.sub(r'^FONTBOUNDINGBOX .*$', f'FONTBOUNDINGBOX {right-left} {top-bottom} {left} {bottom}', header, flags=re.M)
        for metric in ['FONT_ASCENT', 'FONT_DESCENT']:
            value = max(int(re.search(r'^'+metric+r' (\d+)', h, re.M)[1]) for h,_ in variants.values())
            header = re.sub(r'^'+metric+r' \d+', f'{metric} {value}', header, flags=re.M)
        result = header + f'CHARS {len(glyphs)}\n' + ''.join(glyphs) + 'ENDFONT\n'
        name = stem + '-mixed.bdf'
        (directory / name).write_text(result)
        generated[name] = hashlib.sha256(result.encode()).hexdigest()
        print(name, len(glyphs), 'glyphs')
    (directory / 'mixed.json').write_text(json.dumps({'unicode': '17.0.0', 'han_default': 'zh_hans', 'generator': 'scripts/merge-script-fonts.py', 'files': generated}, indent=2)+'\n')

if __name__ == '__main__':
    for name in ['fusion', 'jelly', 'ark']:
        build(ASSETS / f'TakWolf--{name}-pixel-font')
