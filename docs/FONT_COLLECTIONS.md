# Bundled bitmap font selection

Appearance provides family, face, native size and integer scale controls in both
compositors. The script control is removed. Kirsch and Kissinger2 are no longer
bundled; saved selections fall back to Unifont without changing colours/opacity.
Checksum-matched installed copies are backed up and retired on installation;
personal fonts and modified installed files are preserved.

Fusion Pixel, Ark Pixel and Jelly Pixel each expose one combined BDF per native
size and spacing mode. Their original regional sources are retained, compressed,
with upstream hashes and OFL notices. `scripts/merge-script-fonts.py` reproduces
the combined strikes offline from those sources and Unicode 17 `Scripts.txt`.
Each Unicode code point is processed once; source pixels, advances and bearings
are copied verbatim. The font bounding box encloses the union of source glyphs.

Unicode Script selects Japanese for Hiragana/Katakana, Korean for Hangul,
Taiwanese for Bopomofo, Simplified Chinese for Han and Latin for other scripts.
When the preferred source lacks a glyph, the stable fallback order is Latin,
Simplified Chinese, Traditional Chinese, Taiwanese, Hong Kong, Japanese, Korean.
**Unicode unified Han code points do not encode regional glyph preferences.**
Mixed Japanese/Chinese text therefore uses the same Han form unless a separate
contextual shaping system supplies language information. This merge does not
claim to infer a language from a shared Han character or implement variation
sequences. It does preserve the full union of encoded upstream glyphs.

Existing Fusion Pixel selections with a language suffix migrate to their
combined size/spacing equivalent. The physical BDF is shared by the compositor
and exported session fonts; no per-character script setting is required.

Sources:

- [Fusion Pixel](https://github.com/TakWolf/fusion-pixel-font), 2026.10.16: 8/10/12px, mono/proportional.
- [Ark Pixel](https://github.com/TakWolf/ark-pixel-font), 2026.10.25: 10/12px, mono/proportional.
- [Jelly Pixel](https://github.com/TakWolf/jelly-pixel-font), 2026.10.25: 12px, mono/proportional. Incomplete upstream coverage uses the normal missing-glyph fallback.
- [Retro Pixel](https://github.com/TakWolf/retro-pixel-font), 2026.09.18: Arcade, Cute Mono, Cute Prop, Petty 5H, Petty 5x5 and Thick faces.
- [MaruMinya](https://github.com/hicchicc/x12y12pxMaruMinya): original non-Hangul 12px face, alongside the existing MaruMinyaHangul. Pinned revision and conversion details are in `assets/fonts/hicchicc--maruminya/upstream.json`. Embedded bitmaps are preserved; glyphs absent from that strike are explicitly rasterized on the original 12px design grid, without hinting or antialiasing. Every BDF glyph is verified against its source pixels and metrics.

Removing Kirsch also retires its partial Nerd Font private-use coverage.
Powerline remains available in Galmuri, and supplementary emoji in Unifont Upper.
The audit reports individual sample coverage rather than claiming all Unicode
symbols are present.

Validation: `scripts/test-mixed-script-fonts.py`,
`uv run --with freetype-py scripts/audit-bitmap-fonts.py`,
`scripts/test-retired-fonts.py`, Rust library tests,
`scripts/test-bdf-fonts.cpp`, and the isolated compositor appearance cycling test.
