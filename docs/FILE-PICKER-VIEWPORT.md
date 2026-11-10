# Shared FLTK picker on compact viewports

The canonical FLTK 1.4.5 patch makes the shared chooser usable at 640×360 with 32px
bitmap text. The previous 24-patch runtime acknowledged an actual clipboard
filename, then displayed a 960×600 client outside that viewport. Its 720×420
minimum also prevented resizing to the available work area. The unchanged input
behavior and strict geometry negative remain recorded.

The 25th patch replaces only the private chooser/Favorites window implementations,
strictly before their first show. It transfers the original children, callback,
data, modality and window class; public headers and class layout stay unchanged.
Filename, OK/Save and Cancel have a fixed, font-sized footer. The file browser
resizes, the preview remains available, and details retain horizontal scrolling.
Compact layouts expose all original controls in a scrollable Options dialog;
Locations temporarily hosts the original Places group with its actual Add and
Manage actions. Owned Close/WM-close restores the group without navigation.
Selecting a real location dismisses modal descendants before the original
navigation callback focuses the chooser, avoiding a Wayland parent cycle.

The shared `fl_input` used by New Directory and Custom Filter now measures its
selected input font and retains a 25px minimum. It receives no private replacement
or new public field. Resizing/font reflow and repeated Options opening preserve
filenames, multiselection, preview and persisted sorting/detail/Places settings.
Custom filters synchronize the original and Options menus.

The final matrix uses fresh matching test consumers and owned optimized/toolkit
sanitizer builds, with the canonical 53b bitmap runtime. All eight commands pass:

| Backend | Viewport / initial bitmap pixels | Release | ASan/UBSan/leaks |
| --- | --- | ---: | ---: |
| X11 | 1600×1000 / 16 | 35.878s | 52.898s |
| X11 | 640×360 / 32 | 48.446s | 65.976s |
| Native Wayland | 1600×1000 / 16 | 41.177s | 57.668s |
| Native Wayland | 640×360 / 32 | 55.131s | 73.680s |

Each cohort's 1,503 captured source, binary, toolkit, bitmap, compositor and private
tool inputs match before/after. Sanitizer builds instrument all owned toolkit
C/C++ with `-O1 -g1 -fsanitize=address,undefined -fno-omit-frame-pointer`, including
vendored libdecor, plus the test client. External canonical bitmap libraries are
not relabeled as newly instrumented. Leak detection is enabled, with no
suppressions. Release uses `-O3 -DNDEBUG -march=native -mtune=native`.

Checks include the existing sorting/column/Favorites/XDG/bookmark/preview/prefs
and Unicode/multiselection oracles; every Options row fully scrolled into view;
repeated opening resets scroll position; live 16↔32 bitmap reflow; section rows
and owned Close without navigation; actual Favorites deletion Cancel/reopen;
Unicode New Folder and Custom Filter accept/Cancel; and native open, multiple,
save, directory and Cancel results. Captures are observational; control bounds,
glyph height and exact callback/file/selection results are asserted separately.
All 165 public include files match the old prefix, no old core export is removed,
and `sizeof(Fl_File_Chooser)` remains 6328. A client compiled with old 24-patch
headers dynamically loads the final 25-patch runtime and passes compact controls.

Preserved intermediate negatives include repeated Options origin drift, 25px
shared input clipping, stale font-change footer metrics and the actual compact
Wayland parent-cycle refusal. They are not counted as final passes. A final
same-fixture/old-runtime control still exits 1 after filename acknowledgment at
960×600 outside 640×360. Its capture-only wrapper exits 0; the receipt explicitly
records the actual native exit 1 separately.

Evidence is rooted at
`/home/blux/de-xr/DomeD/artifacts/fltk-picker-viewport-53b/artifacts/picker-proof`;
[the receipt](validation/FILE-PICKER-VIEWPORT.json) pins logs, full before/after
ledgers and runtime/test hashes. The private Xvfb wrapper uses signature-verified
extracted RPMs in an unprivileged bwrap namespace. Native Wayland uses a private
nested compositor, resized on that private X display, with login/session services
disabled and a fail-closed XR runtime. The compositor keeps the Release toolkit
when running a sanitizer client. No user desktop, physical headset, hardware GPU,
system package/install or session state was changed. DomeD main remains the
untouched user stereo commit a5c444c.

Reproduction uses the existing canonical builder with explicit new prefixes:

```sh
PKG_CONFIG_PATH=/path/to/canonical/bitmap/lib/pkgconfig \
  python3 scripts/prepare-fltk-experiment.py \
  --deps artifacts/picker-deps-release --prefix artifacts/toolkit-picker-final --jobs 4
# Use distinct deps/prefix and --sanitize for the owned sanitizer build.
PKG_CONFIG_PATH=/path/to/canonical/bitmap/lib/pkgconfig \
  cmake -S tests/file-picker -B artifacts/picker-test-release \
  -DCMAKE_BUILD_TYPE=Release -DFLTK_DIR=/path/to/toolkit/share/fltk
cmake --build artifacts/picker-test-release --parallel 4
```

For native Wayland, put the private Xvfb wrapper on PATH, select the private
compositor through `DOME_COMPOSITOR`, set canonical toolkit/bitmap
`LD_LIBRARY_PATH`, and run `tests/file-picker/test-wayland.py BINARY OUTPUT`.
`DOME_PICKER_VIEWPORT=640x360 DOME_PICKER_FONT_SCALE=2` selects the compact case;
`DOME_PICKER_COMPOSITOR_LIBRARY_PATH` keeps a Release compositor SDK while the
client uses the owned sanitizer toolkit. The receipt stores the exact commands.
This is a canonical dialog component qualification, not a new Sculpt frontend or
an arbitrary-tiny-workarea/physical-device claim.
