# Canonical FLTK process cleanup checkpoint

A subsequent canonical Cairo source-cache fix now removes the optional X11
800-byte report when using the matching new bitmap runtime. Its whole-toolkit/
client sanitizer fixture passes9 checks on each backend with clean exits. Read
[CAIRO-XLIB-CACHE-CLEANUP.md](CAIRO-XLIB-CACHE-CLEANUP.md). The parent evidence
below remains scoped to the earlier bitmap runtime and retains its negatives.

The FLTK1.4.5 builder now applies `fltk-1.4.5-process-cleanup.patch`. It releases
the compose table's constructor reference after creating the keyboard state,
destroys the GTK-shell protocol proxy on explicit display close, and releases
owned Cairo Clip nodes, regions and dummy context when a driver is destroyed.
The real window's Cairo context remains borrowed. The exit callback checks
whether an explicit close already deleted the screen driver. No automatic global
display teardown is introduced: applications must destroy every widget, GL
resource and worker before calling `fl_close_display`, then call no FLTK API.

Whole-toolkit sanitizer instrumentation also exposed virtual calls from plugin
base initializers before the corresponding base object was constructed. The
OpenGL-device and Wayland plugin constructors now pass their unchanged class
and registration names directly. No virtual signature or class layout changes.

The new `test-fltk-process-cleanup.py` fixture uses actual native windows, GL
contexts, image surfaces, bitmap text and persistent clipping regions. Three
hide/show cycles qualify9 checks per backend. Release X11+native Wayland18 pass.
Whole FLTK/vendored-libdecor/client C/C++ O1 ASan/UBSan with leak detection passes
native Wayland9 and exits0, with no suppression. All1,076 captured inputs remain
exact. Runtime maps verify the staged FLTK and canonical bitmap FreeType; this
minimal fixture does not load HarfBuzz or claim a full text-shaping workflow.

An earlier diagnostic wrapper around the actual Dome Sculpt application calls
the documented final display close after normal main teardown. The old toolkit
reports159,565 bytes/699 allocations; the initial cleanup patch exits0. This is
a diagnostic, not production application integration. The later fully
instrumented toolkit fixture separately qualifies the plugin-constructor fixes.
Final builds retain matching unchanged cached translation units; initial fresh
builds and subsequent source-triggered recompiles are recorded separately.

Optional X11 remains an explicit negative: its9 functional checks complete, then
LSan reports800 bytes/two allocations in Cairo's Xlib `source` helper. The exact
same fixture with the old canonical toolkit reports968 bytes/eight allocations:
the same800-byte Cairo report plus168 bytes in Clip/region ownership. The patch
removes the latter; it does not resolve the former. This is not an all-backends
sanitizer pass, and standalone-XR's separate Mesa112-byte report is unchanged.

Exact commands, builds, source/binary hashes, negative reports and setup
corrections are in [the receipt](validation/FLTK-PROCESS-CLEANUP.json). Prefixes
are privately staged as the regular user. No system installation, active session
restart, physical headset or GPU device use occurred. Dome Sculpt still needs
its production display-owner integration and current-application qualification.
