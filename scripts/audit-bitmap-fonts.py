#!/usr/bin/env python3
"""Audit bundled strikes, provenance and actual glyphs. Run with uv --with freetype-py.

Coverage means every character in the named sample exists in this face, without
fallback. It does not assert that an entire Unicode block or language is covered.
"""
import hashlib
import json
from pathlib import Path
import re
import freetype

root = Path(__file__).resolve().parents[1]
assets = root / 'assets/fonts'
samples = {
    'Czech': 'Příliš žluťoučký kůň úpěl ďábelské ódy. ĚŠČŘŽÝÁÍÉŮÚŤĎŇ',
    'German': 'Falsches Üben von Xylophonmusik quält jeden größeren Zwerg. ÄÖÜäöüßẞ',
    'Cyrillic': 'Русский Українська Български Српски Ђђ Ћћ Љљ Њњ Џџ Ѓѓ Ќќ',
    'Japanese': '日本語 あいうえお カタカナ',
    'Korean': '한국어 한글 가나다',
    'Chinese simplified': '简体中文 汉语 龙门',
    'Chinese traditional': '繁體中文 漢語 龍門',
    'Thai': 'ภาษาไทย สวัสดี กำ กิ กี่ กุ กู',
    'Arabic': 'العربية مرحبا بالعالم لا لأ لإ لآ',
    'Emoji': '😀😂🙂🚀🐧🔒📁',
    'Nerd Font Material sample': '\U000f0001\U000f0006',
    'Powerline': '\ue0a0\ue0a1\ue0a2\ue0b0\ue0b1\ue0b2\ue0b3',
    'Nerd Font common': '\ue5ff\ue62b\ue702\uf17c',
    'Nerd Font extended sample': '\ue5ff\ue62b\ue702\uf013\uf07c\uf121\uf17c\uf120\uf15b',
}
paths = re.findall(r'bitmap!\("([^"]+)"\)', (root / 'src/bundled_fonts.rs').read_text())
assert paths and len(paths) == len(set(paths))
records = []
for relative in paths:
    path = assets / relative
    face = freetype.Face(str(path))
    assert not face.is_scalable, f'{relative}: outline face'
    assert face.has_fixed_sizes, f'{relative}: missing fixed strikes'
    face.select_charmap(freetype.FT_ENCODING_UNICODE)
    sizes = sorted({round(s.y_ppem / 64) for s in face.available_sizes})
    assert all(4 <= s <= 64 for s in sizes), (relative, sizes)
    text_face = bool(face.get_char_index(ord('A')))
    coverage = {name: all(face.get_char_index(ord(c)) for c in text) for name, text in samples.items()}
    icons = sum(bool(face.get_char_index(cp)) for cp in range(0xe000, 0xf900))
    icons += sum(bool(face.get_char_index(cp)) for cp in range(0xf0000, 0xf2000))
    record = {'file': relative, 'family': face.family_name.decode(), 'style': face.style_name.decode(),
              'sizes': sizes, 'selectable_text_face': text_face, 'glyphs': face.num_glyphs, 'private_use_glyphs': icons, 'samples': coverage}
    records.append(record)
    print(f'{relative}: {sizes} px; ' + ', '.join(k for k, v in coverage.items() if v))

for manifest in assets.glob('*/upstream.json'):
    for name, expected in json.loads(manifest.read_text())['files'].items():
        assert hashlib.sha256((manifest.parent / name).read_bytes()).hexdigest() == expected, (manifest, name)
info = json.loads((assets / 'upstream.json').read_text())
assert hashlib.sha256((assets / 'unifont-17.0.04.bdf').read_bytes()).hexdigest() == info['sha256_uncompressed']
for sample in samples:
    if sample.startswith('Nerd Font'): continue # Kirsch's private-use icons were retired with the font
    assert any(r['samples'][sample] for r in records), f'No bundled face covers {sample}'
distributed = {str(p.relative_to(assets)) for p in assets.rglob('*')
               if p.suffix == '.bdf' and p.name != 'upstream.bdf'}
assert set(paths) == distributed, 'Bundled registry and distributed fonts differ'
notices = (root / 'packaging/font-notices.list').read_text().splitlines()
assert len(notices) == len(set(notices))
expected_notices = {str(p.relative_to(assets)) for p in assets.rglob('*')
                    if p.is_file() and p.suffix != '.bdf'}
assert set(notices) == expected_notices, 'Distribution must include every font notice and conversion script'
assert any(r['family'] == 'x10y12pxDenkiChipHangul' and r['samples']['Korean'] for r in records)
allowed=set(json.loads((assets/'collections.json').read_text())['families'])
def family(name):
    name=name.split(' [')[0]
    if name.startswith('Galmuri'):return 'Galmuri'
    for prefix in ['Fusion Pixel', 'Jelly Pixel', 'Ark Pixel', 'Retro Pixel']:
        if name.startswith(prefix):return prefix
    if name.startswith('Neep'):return 'Neep'
    if name.startswith('Kissinger 2'):return 'Kissinger2'
    return re.sub(r'^x\d+y\d+px', '', {'qub':'QUB'}.get(name,name))
assert {family(r['family']) for r in records} == allowed, 'Only the requested default families may be bundled'
assert all(r['file'].endswith('.bdf') for r in records)
assert any(r['family'] == 'x12y12pxMaruMinya' for r in records)
assert not any(r['family'] == 'x12y12pxMaruMinyaM' for r in records)
assert {r['style'] for r in records if r['family'] == 'Tekuplus'} == {'Regular', 'Bold'}
# The upper-plane BDF must preserve every native source bitmap exactly.
upper = assets / 'unifont-upper-17.0.04'
source = {int(line.split(':')[0],16):line.split(':')[1].upper()
          for line in (upper/'unifont_upper-17.0.04.hex').read_text().splitlines()}
packaged = {}
for block in (upper/'unifont_upper-17.0.04.bdf').read_text().split('STARTCHAR ')[1:]:
    cp = int(re.search(r'^ENCODING (\d+)$', block, re.M)[1])
    packaged[cp] = re.split(r'BITMAP[ \t]*\n', block)[1].split('ENDCHAR')[0].replace('\n','').upper()
assert source == packaged, 'Unifont Upper must retain every native HEX glyph pixel'
assert not any(family(r['family']) in {'QUB','Neep','Cozette','uni16m','Zpix','kirsch','Kissinger2'} for r in records)
out = root / 'artifacts/bitmap-fonts'
out.mkdir(parents=True, exist_ok=True)
(out / 'coverage.json').write_text(json.dumps({'samples': samples, 'fonts': records}, ensure_ascii=False, indent=2) + '\n')
print(f'PASS: {len(records)} bitmap files, BDF-only native strikes, pinned checksums, all requested script samples covered')
