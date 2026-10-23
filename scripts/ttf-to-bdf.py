#!/usr/bin/env python3
"""Convert a pixel TTF to one Unicode BDF strike; verify every glyph after writing.

Run with uv run --no-project --with fonttools --with freetype-py python.
Embedded monochrome strikes are extracted losslessly. Outline-only pixel fonts
are rasterized without hinting or antialiasing at their original design size.
This is an offline packaging tool, never a runtime font fallback.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re

import freetype as ft
from fontTools.ttLib import TTFont


def bitmap(face):
    b = face.glyph.bitmap
    if b.width and b.rows and b.pixel_mode != ft.FT_PIXEL_MODE_MONO:
        raise ValueError('Expected a one-bit bitmap')
    stride = abs(b.pitch)
    raw = b.buffer
    rows = []
    for y in range(b.rows):
        offset = (y if b.pitch >= 0 else b.rows - 1 - y) * stride
        row = bytearray(raw[offset:offset + (b.width + 7) // 8])
        if row and b.width % 8:
            row[-1] &= 0xff << (8 - b.width % 8) & 0xff
        rows.append(bytes(row).hex().upper())
    if face.glyph.advance.x % 64 or face.glyph.advance.y:
        raise ValueError('Non-integral horizontal advance')
    return (b.width, b.rows, face.glyph.bitmap_left,
            face.glyph.bitmap_top - b.rows, face.glyph.advance.x // 64, rows)


def convert(source, output, pixels, family=None, allow_outline_supplement=False):
    source, output = Path(source), Path(output)
    sha = hashlib.sha256(source.read_bytes()).hexdigest()
    face = ft.Face(str(source))
    face.select_charmap(ft.FT_ENCODING_UNICODE)
    strikes = [i for i, size in enumerate(face.available_sizes)
               if size.y_ppem == pixels * 64]
    embedded = bool(strikes)
    if embedded:
        face.select_size(strikes[0])
        flags = ft.FT_LOAD_SBITS_ONLY | ft.FT_LOAD_RENDER | ft.FT_LOAD_MONOCHROME | ft.FT_LOAD_TARGET_MONO
    else:
        face.set_pixel_sizes(0, pixels)
        flags = (ft.FT_LOAD_NO_BITMAP | ft.FT_LOAD_NO_HINTING | ft.FT_LOAD_NO_AUTOHINT |
                 ft.FT_LOAD_RENDER | ft.FT_LOAD_MONOCHROME | ft.FT_LOAD_TARGET_MONO)
    family = family or face.family_name.decode('utf-8')
    style = face.style_name.decode('utf-8')
    with TTFont(source) as font:
        def name(number):
            return font['name'].getDebugName(number) or ''
        copyright, version = name(0), name(5)
    glyphs = {}
    supplemented = []
    for cp, index in face.get_chars():
        if not index or cp > 0x10ffff or 0xd800 <= cp <= 0xdfff:
            continue
        try:
            face.load_char(cp, flags)
        except ft.FT_Exception:
            if not embedded or not allow_outline_supplement:
                raise
            face.load_char(cp, ft.FT_LOAD_NO_BITMAP | ft.FT_LOAD_NO_HINTING |
                           ft.FT_LOAD_NO_AUTOHINT | ft.FT_LOAD_RENDER |
                           ft.FT_LOAD_MONOCHROME | ft.FT_LOAD_TARGET_MONO)
            supplemented.append(cp)
        glyphs[cp] = bitmap(face)
    if not glyphs:
        raise ValueError('Empty Unicode font')
    visible = [g for g in glyphs.values() if g[0] and g[1]]
    left = min(g[2] for g in visible)
    bottom = min(g[3] for g in visible)
    right = max(g[2] + g[0] for g in visible)
    top = max(g[3] + g[1] for g in visible)
    # Preserve the source baseline/line metrics; BBX retains overshooting accents.
    ascent = max(0, face.size.ascender // 64)
    descent = max(0, -face.size.descender // 64)
    if ascent + descent == 0:
        ascent, descent = max(0, top), max(0, -bottom)
    spacing = 'M' if len({g[4] for g in glyphs.values() if g[4]}) == 1 else 'P'
    average = round(sum(g[4] for g in glyphs.values()) * 10 / len(glyphs))
    quote = lambda value: '"' + value.replace('"', '""').replace('\n', ' ') + '"'
    props = {'FOUNDRY': quote('DomeD'), 'FAMILY_NAME': quote(family),
             'WEIGHT_NAME': quote('Bold' if 'bold' in style.lower() else 'Regular'),
             'SLANT': quote('R'), 'SETWIDTH_NAME': quote('Normal'),
             'ADD_STYLE_NAME': quote(''), 'PIXEL_SIZE': pixels,
             'POINT_SIZE': pixels * 10, 'RESOLUTION_X': 72, 'RESOLUTION_Y': 72,
             'SPACING': quote(spacing), 'AVERAGE_WIDTH': average,
             'CHARSET_REGISTRY': quote('ISO10646'), 'CHARSET_ENCODING': quote('1'),
             'FONT_ASCENT': ascent, 'FONT_DESCENT': descent,
             'COPYRIGHT': quote(copyright), 'FONT_VERSION': quote(version)}
    xfamily = re.sub(r'[^A-Za-z0-9_]', '', family)
    weight = 'bold' if 'bold' in style.lower() else 'medium'
    method = 'embedded monochrome strike' if embedded else 'unhinted monochrome outline rasterization'
    if supplemented:
        method += ' with explicit outline-only Unicode supplements'
    lines = ['STARTFONT 2.1', f'COMMENT DomeD: {method}; source SHA256 {sha}',
             f'FONT -domed-{xfamily}-{weight}-r-normal--{pixels}-{pixels * 10}-72-72-{spacing.lower()}-{average}-iso10646-1',
             f'SIZE {pixels} 72 72', f'FONTBOUNDINGBOX {right-left} {top-bottom} {left} {bottom}',
             f'STARTPROPERTIES {len(props)}']
    lines += [f'{key} {value}' for key, value in props.items()]
    lines += ['ENDPROPERTIES', f'CHARS {len(glyphs)}']
    for cp, (width, height, x, y, advance, rows) in glyphs.items():
        lines += [f'STARTCHAR U+{cp:04X}', f'ENCODING {cp}',
                  f'SWIDTH {round(advance * 1000 / pixels)} 0', f'DWIDTH {advance} 0',
                  f'BBX {width} {height} {x} {y}', 'BITMAP', *rows, 'ENDCHAR']
    lines += ['ENDFONT', '']
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text('\n'.join(lines), encoding='utf-8')
    check = ft.Face(str(output))
    assert not check.is_scalable and check.has_fixed_sizes
    assert check.family_name.decode() == family
    assert check.available_sizes[0].y_ppem == pixels * 64
    check.select_charmap(ft.FT_ENCODING_UNICODE)
    check.select_size(0)
    assert {cp for cp, index in check.get_chars() if index} == set(glyphs)
    for cp, expected in glyphs.items():
        check.load_char(cp, ft.FT_LOAD_DEFAULT)
        assert bitmap(check) == expected, f'{output}: U+{cp:04X} pixels or metrics changed'
    return {'source_sha256': sha, 'pixels': pixels, 'method': method,
            'family': family, 'style': style, 'copyright': copyright,
            'outline_supplement_codepoints': supplemented,
            'glyphs': len(glyphs), 'all_glyphs_verified': True,
            'sha256': hashlib.sha256(output.read_bytes()).hexdigest()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--pixels', required=True, type=int)
    parser.add_argument('--family')
    parser.add_argument('--allow-outline-supplement', action='store_true',
                        help='Explicitly rasterize glyphs missing from a partial embedded strike')
    args = parser.parse_args()
    if not 4 <= args.pixels <= 64:
        parser.error('Native design size must be between 4 and 64 pixels')
    print(json.dumps(convert(args.source, args.output, args.pixels, args.family,
                             args.allow_outline_supplement), indent=2))
