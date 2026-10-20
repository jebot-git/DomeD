#!/usr/bin/env python3
"""Rasterize the public-domain original at its intended 16px size.

Download/extract the TTF zip from https://cactus.tistory.com/193, then run:
uv run --no-project --with freetype-py python convert.py /path/to/DungGeunMo.ttf
No outlines or antialiased pixels are included in the generated BDF.
"""
import hashlib
from pathlib import Path
import sys
import freetype

source = Path(sys.argv[1])
assert hashlib.sha256(source.read_bytes()).hexdigest() == '3f184d949149f2e7e6a853593e25161fb1d3b6d3708ebf79ce78600788fdea30'
face = freetype.Face(str(source))
face.select_charmap(freetype.FT_ENCODING_UNICODE)
face.set_pixel_sizes(0, 16)
glyphs = []
left = bottom = 0
right = top = 0
for cp, index in face.get_chars():
    if cp > 0xffff or not index:
        continue
    face.load_glyph(index, freetype.FT_LOAD_RENDER | freetype.FT_LOAD_TARGET_MONO)
    slot = face.glyph
    bitmap = slot.bitmap
    x, y = slot.bitmap_left, slot.bitmap_top - bitmap.rows
    left, right = min(left, x), max(right, x + bitmap.width)
    bottom, top = min(bottom, y), max(top, slot.bitmap_top)
    assert bitmap.pitch >= 0
    rows = [bytes(bitmap.buffer[row * bitmap.pitch:row * bitmap.pitch + (bitmap.width + 7) // 8]).hex().upper()
            for row in range(bitmap.rows)]
    advance = round(slot.advance.x / 64)
    glyphs.append(f'STARTCHAR uni{cp:04X}\nENCODING {cp}\nSWIDTH {round(advance * 1000 / 16)} 0\n'
                  f'DWIDTH {advance} 0\nBBX {bitmap.width} {bitmap.rows} {x} {y}\nBITMAP\n'
                  + '\n'.join(rows) + '\nENDCHAR\n')
properties = [
    'FONTNAME_REGISTRY ""', 'FOUNDRY "DomeD"', 'FAMILY_NAME "DungGeunMo"',
    'WEIGHT_NAME "Medium"', 'SLANT "R"', 'SETWIDTH_NAME "Normal"',
    'ADD_STYLE_NAME ""', 'PIXEL_SIZE 16', 'POINT_SIZE 160',
    'RESOLUTION_X 72', 'RESOLUTION_Y 72', 'SPACING "C"', 'AVERAGE_WIDTH 80',
    'CHARSET_REGISTRY "ISO10646"', 'CHARSET_ENCODING "1"',
    f'FONT_ASCENT {top}', f'FONT_DESCENT {-bottom}',
    'COPYRIGHT "Public domain. Jungtae Kim, Darien Gavin Valentine, Kil Hyung-jin (CACTUS)."',
]
header = ('STARTFONT 2.1\n'
          'FONT -DomeD-DungGeunMo-Medium-R-Normal--16-160-72-72-C-80-ISO10646-1\n'
          f'SIZE 16 72 72\nFONTBOUNDINGBOX {right-left} {top-bottom} {left} {bottom}\n'
          f'STARTPROPERTIES {len(properties)}\n' + '\n'.join(properties)
          + f'\nENDPROPERTIES\nCHARS {len(glyphs)}\n')
destination = Path(__file__).with_name('DungGeunMo.bdf')
destination.write_text(header + ''.join(glyphs) + 'ENDFONT\n')
print(f'Generated {destination.name}: {len(glyphs)} glyphs, native 16px, FreeType {freetype.version()}')
