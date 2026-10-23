GNU Unifont Upper 17.0.04

Supplementary-plane emoji, pictographs, historic scripts, musical and other
symbols at their native 16px bitmap strike. Built from upstream precompiled HEX
bitmaps using the upstream hex2bdf Perl tool, following font/Makefile. No outline
rasterization or glyph resampling. Monochrome single-codepoint glyphs; this does
not provide color emoji or composed emoji sequences.

Rebuild: perl hex2bdf --font "Unifont Upper" --version 17.0.04 \
  --copyright '<COPYRIGHT from the bundled Unifont BDF>' \
  unifont_upper-17.0.04.hex > unifont_upper-17.0.04.bdf

Font license: OFL-1.1 OR GPL-2.0-or-later WITH Font-exception-2.0.
The upstream hex2bdf tool is GPL-2.0-or-later; see COPYING.
See also ../COPYING.unifont, ../OFL-1.1.txt and ../README.unifont.
