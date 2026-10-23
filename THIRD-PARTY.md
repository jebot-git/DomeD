# Third-party source

The font bundle contains BDF resources only. GNU Unifont 17.0.04 is distributed
under SIL OFL 1.1; its full notices, original README and provenance remain in
`assets/fonts/`. LightXR separately retains its pre-session PCF login font.

Collection-specific licenses, embedded copyright notices, source revisions,
original and packaged SHA-256 checksums, and packaging details are retained in
`assets/fonts/*/upstream.json` and the adjacent source documentation. See the
font section in README.md for the curated bundle and format policy. QUB, Neep,
Cozette, uni16m and Zpix have been retired from the default distribution.
Kissinger2 dev5 is distributed as the two unmodified upstream BDFs, with the author's
public-domain dedication in `assets/fonts/kissinger2-dev5/readme.txt`.
kirsch v0.7.3 is the unmodified upstream BDF, licensed under OFL-1.1; its Nerd Fonts
coverage is partial. Unifont Upper 17.0.04 is built from the original upstream HEX
bitmaps using the accompanying GPL-2.0-or-later `hex2bdf` tool, preserving every
bitmap pixel. Its font terms are OFL-1.1 OR GPL-2.0-or-later WITH Font-exception-2.0.
No outline rasterization is used for either addition. Source files, notices and
checksums accompany both additions.

Tekuplus Regular/Bold, MaruMinyaHangul, ShijimiSerif and the 00ff collection are
also included as BDF families. Tekuplus retains its M+ permission notice;
MaruMinyaHangul, ShijimiSerif, NegaTape, NegaClip and DenkiChip retain their
upstream OFL-1.1 notices. The nine older 00ff TTF conversions retain hicc’s
original license, including its restrictions on selling fonts and claiming
authorship; they are not distributed under OFL. See
`assets/fonts/pixel-font-imports.md` and per-collection manifests for design
sizes, embedded bitmap extraction, explicit outline supplements and complete
source-to-BDF glyph verification.

BDF packaging maps legacy character codes to Unicode and normalizes metadata;
glyph bitmap pixels remain unchanged.
`packaging/font-notices.list` installs the accompanying terms and provenance.
The generated `fonts.index` accelerates discovery; runtime-modified fonts are
inspected independently.

`native/bdf2x/bdf2x.c` is Francesco149/bdf2x at commit
`68055df8fba05333386f9f74929fa36cda23bf55`, under the Unlicense. The shared
renderer adapts its integer pixel replication, additionally updating BDF metrics
and family identity. Source, license and adaptation notes are retained together.

The FLTK 1.4.5 `freetype-only.patch` replaces its Cairo font catalogue,
descriptors and measurements with Fontconfig/FreeType and removes Pango links.
`freetype-decor.patch` uses the same renderer in the bundled Cairo decoration
plugin and prevents loading external GTK/Pango decoration plugins.
The complete pinned FLTK source and patch series are retained by its build script.

For GTK and WebKit, the private Pango 1.57.0 build adds native BDF metrics through FreeType-backed
HarfBuzz callbacks. `native/toolkit-hooks/pango-1.57.0-native-bdf.patch` retains
the changes; `scripts/prepare-pango-bdf.py` verifies the official source archive
and installs the source, patch and LGPL COPYING alongside the private libraries.
Distribution libraries are not replaced.

`protocols/gtk-shell.xml` is the GTK Wayland extension from GNOME GTK's
`gtk-3-24` branch (`gdk/wayland/protocol/gtk-shell.xml`). GTK's library license
is retained in `protocols/COPYING.gtk`; its generated protocol bindings enable
interoperability with GTK's application-menu exports.

`native/toolkit-hooks/fltk-1.4.4-popup-done.patch` modifies upstream FLTK 1.4.4's
Wayland menu dismissal. The optional build script verifies the upstream archive
and retains its complete source, patch and COPYING alongside the user-local
runtime. FLTK is LGPL-2.0-or-later with its upstream linking exceptions. The
runtime is separately built and dynamically loaded by FLTK applications; it is
not linked into DomeD. See `native/toolkit-hooks/README.md`.

The initial nested Wayland plumbing in `src/bin/dome-compositor/` is adapted
from Smithay's MIT-licensed Smallvil example, tag v0.7.0, commit
`a166cf4c94b5aedc332a65aa1dd753e8148829c3`.
Copyright (c) 2017 Victor Berger and Victoria Brekenfeld.
The original MIT license is retained in LICENSE.

The native DRM, GBM and libseat/libinput integration follows Smithay's
MIT-licensed Anvil example from the same v0.7.0 release.

Dome's scene model, shell, control service, preview and tests are new code.
No WiVRn-Pro source is imported into this repository.

`native/toolkit-hooks/miro-0.12.0-dome-theme.patch` modifies the optional
AGPL-3.0 Miro PDF viewer at upstream commit
`d35322c47d53903638558162a5e3153f32dfdc37` from
https://github.com/vincent-uden/miro. The local installer retains upstream
source, license, revision and patch. Miro is separately built and installed;
it is not linked into DomeD.

The Vulkan/OpenXR initialization in `src/bin/dome-xr.rs` is adapted from the
MIT/Apache-2.0 openxrs `openxr` 0.21.1 Vulkan example (Ralith contributors).
Qt6 is dynamically linked and supplies its own LGPL/GPL/commercial licensing;
Qt source is not included. See dependency distributions for license terms.

The separately executed native authentication helper is GnuPG Pinentry 1.3.3
(GPL-2.0-or-later), from
https://www.gnupg.org/ftp/gcrypt/pinentry/pinentry-1.3.3.tar.bz2.
SHA-256: `c2970f16d6afb66ecddfca767d743936c86239bff936eed7fd7597a678414b63`.
The upstream signature was verified with the system GnuPG distribution keyring,
Werner Koch distribution signing key `6DAA6E64A76D2840571B4902528897B826403ADA`.
`native/pinentry/` supplies GPL-2.0-or-later integration code and patches; the
shared `native/fltk_appearance.h` retains its MIT license. The helper dynamically
links FLTK and remains a separate process from the MIT-licensed compositor.
Its corresponding upstream source, modifications and license are staged with it.

`native/tray/desktop-menu.{cpp,h}`, `appmenu.xml` and `desktop-menu.cmake` reuse
the MIT-licensed de-xr FLTK universal-menu exporter from the cmuxr frontend.
The service manager talks to KDE Connect, Fcitx5 and KWallet over their public
D-Bus interfaces; it does not incorporate or link their Qt frontends.

## Optional FLTK translator

`native/translator` runs a separate Python worker using pykakasi 2.3.0
([upstream](https://github.com/miurahr/pykakasi), GPL-3.0-or-later) and its Japanese
reading dictionaries. Its pinned dependencies are jaconv 0.5.0 (MIT), Deprecated
1.3.1 (MIT) and wrapt 2.0.1 (BSD-2-Clause). Their wheel metadata and license files
are retained in the installed worker's `vendor/` directory. These are fetched at
build time rather than copied into this source tree. The worker also calls the
system ICU library (Unicode license) for Korean and other script transliteration.
The FLTK application, worker and tray implementation in this repository are MIT.
