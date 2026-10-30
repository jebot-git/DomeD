# Validation: 2026-10-30

Validated on Ubuntu 26.04 amd64 against the published HarfBuzz fork commit
`39144f4a5da0cd0c2ffd9e61649faba4d6d6a2eb`.

- Native BDF loading through blob and FreeType face/font constructors passed
  for Cozette, scientifica and Unifont.
- Unicode coverage, glyph IDs, advances and ink extents matched FreeType.
  LTR/RTL glyph clusters and 1x/2x metrics passed.
- TTF, public table-callback construction and malformed BDF input were rejected;
  rejected FreeType faces invoked their ownership callbacks correctly.
- Paired Pango selected native BDF files and matched direct FreeType advances
  and exact monochrome Cairo glyph pixels for all three fixtures.
- The actual Rust session-export regression passed with the full bundled font
  catalog, including rejection of a TTF renamed to `.bdf`.
- Installed-system FLTK Cairo and OpenGL rendering passed all twelve exact
  bitmap-mask/advance checks: three fonts, two scales and two rendering paths.
  OpenGL used an isolated Xvfb/nested Wayland software renderer.
- The runtime policy check accepted the recorded BDF libraries and rejected
  stock HarfBuzz. The normal user's installed-system check passed.
- Fourteen HarfBuzz/Pango packages were installed as a matched set, without
  removing packages. APT consistency and dpkg audit passed; autoremove was empty.
- The package generator normalizes public files/directories to readable system
  permissions independently of the invoking user's umask.

The Graphite2 outline backend and its two face/font accessors are intentionally
not included. Other tested exported symbols were retained. Complex-script and
non-bitmap application rendering are not acceptance requirements for this fork.

The active desktop was not restarted. Existing processes can retain their
previous mapped libraries until restarted; new logins require the BDF runtime.
