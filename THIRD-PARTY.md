# Third-party source

`assets/fonts/unifont-17.0.04.pcf` is the unmodified Unicode bitmap GNU Unifont
17.0.04 from Unifoundry. It is embedded in the compositor for its global interface
font. Copyright (C) 1998-2026 Roman Czyborra, Paul Hardy and contributors;
the full notice remains in the font. Its OFL-1.1/GPL-2.0-or-later with font
exception licensing, upstream README and provenance are retained beside the
font. Dome distributes the font under the SIL Open Font License 1.1.

`assets/fonts/galmuri-v2.40.4/` contains the upstream bitmap-only Galmuri
7/9/11/14 regular TrueType files by Lee Minseo (quiple), under OFL-1.1.
`assets/fonts/denkichip-v1.213/` contains x10y12pxDenkiChipHangul by Lee Minseo
and the x8y12pxDenkiChip Project Authors, also under OFL-1.1. Its BDF framing,
Unicode charset and 12 px metadata are normalized for FreeType/Qt; glyph data
is unchanged. Both directories retain their licenses and pinned provenance;
DenkiChip also retains the upstream BDF and reproducible normalization script.

`assets/fonts/cozette-v1.30.0/` contains the unmodified Cozette 1.30.0
OpenType bitmap font by the-moonwitch, under the MIT license. Its license and
pinned release provenance with SHA-256 checksums are retained alongside it.

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
