# Pixel font imports

The source revisions and SHA-256 hashes are pinned in each directory’s
`upstream.json`. No TTF is distributed or loaded at runtime. The default font
and saved appearance selection are unchanged. All new faces use the existing
native size and exact 2x controls; design-grid prefixes are hidden in the family
picker while saved physical font identities remain intact.

| Directory | Source data | Native pixels |
| --- | --- | --- |
| itouhiro--tekuplus | TekuplusBit-Regular/Bold TTF, complete family | 12 |
| quiple--maruminya-hangul | upstream fonts/x12y12pxMaruMinyaHangul.bdf | 12 |
| uimss--shijimi-serif | upstream ShijimiSerif-Regular.bdf | 10 |
| hicchicc--negatape | upstream fonts/ttf/x5y8pxNegaTape.ttf | 8 |
| hicchicc--negaclip | upstream fonts/ttf/x5y8pxNegaClip.ttf | 8 |
| hicchicc--denkichip | upstream fonts/ttf/x8y12pxDenkiChip.ttf | 12 |
| hicchicc--00ff | nine TTFs from hicchicc.github.io/00ff | 12–32 |

Tekuplus’s source BDFs are constituent pieces with different encodings, not a
complete Unicode family. Its complete TekuplusBit TTF supplies the original
embedded monochrome strike. The packaged family is called Tekuplus. Missing
bitmap entries are supplemented from the pixel outlines at 12 px. The M+ terms
and Tekuplus notices are preserved. ShijimiSerif’s upstream 10 px BDF includes
its baseline and accent overshoots; its glyphs are not rasterized from the TTF. Its malformed size metadata is
corrected and the family is renamed **DomeD Shijimi** to respect the upstream
ShijimiSerif reserved font name.

Conversion uses FreeType monochrome rendering without hinting, autohinting or
antialiasing. Embedded strikes are requested with SBITS_ONLY; missing glyphs
are errors unless `--allow-outline-supplement` is explicitly supplied. Every
supplement codepoint is recorded. The BDF is then opened with FreeType and
**every** mapped glyph’s pixels, advance and bearings are compared to the source
rendering. Outline conversion runs only during packaging. BDF rendering remains
bitmap-only in DomeD and its clients.

To reproduce, obtain each repository at its manifest revision (the `repository`
field plus `/raw/REVISION/SOURCE` identifies each TTF). The 00ff source paths
include the `00ff/` prefix. Check the `source_sha256` before conversion, then run
from the DomeD checkout, substituting one manifest font record:

```sh
uv run --no-project --with fonttools --with freetype-py python \
  scripts/ttf-to-bdf.py SOURCE.ttf OUTPUT.bdf --pixels PIXELS
```

For Tekuplus add `--family Tekuplus`. When the record has nonempty
`outline_supplement_codepoints`, also add `--allow-outline-supplement`.
Compare the output SHA-256 with the record’s `sha256`. Native BDF imports use
`scripts/import-bdf-collection.py` to normalize Unicode metadata; source glyph
pixels are unchanged. Regenerate `fonts.index` and the notice inventory using
`scripts/index-bdf-fonts.py`, then run `scripts/audit-bitmap-fonts.py` with
freetype-py and compile/run `scripts/test-bdf-fonts.cpp` against Fontconfig,
FreeType and zlib to check application discovery and exact 2x replication.

The website’s older nine fonts use its **original license**, retained verbatim
in `hicchicc--00ff/LICENSE-upstream.html`; they are not relabeled OFL. NegaTape,
NegaClip and DenkiChip have separate upstream OFL notices. MaruMinya and
MaruMinyaM TTFs are deliberately excluded in favor of MaruMinyaHangul. Original
README/license files and font copyright properties accompany the collection.
