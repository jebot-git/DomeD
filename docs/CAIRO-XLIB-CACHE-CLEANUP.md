# Cairo Xlib source-cache cleanup

The canonical Cairo1.18.4 patch now releases the Xlib display's cached white,
alpha and solid color-source surfaces during device finish. Those surfaces have
no device reference and previously survived after their owning display was
removed from the global list. Cleanup runs under the existing device acquisition
and protected X error handler while Display/Render remain usable. Cache slots
are cleared; repeated finish and close retain the ordinary device semantics.
No font/drawing policy, public ABI or application frontend changes.

The independent actual-Xlib regression populates all cache classes through real
source/mask rendering, reads pixels through XGetImage and closes four displays.
Two cases explicitly finish the device twice while context/surface references
still exist. The old canonical runtime reports458,000 leaked bytes in1,145
source objects. The fixed runtime exits0 without a sanitizer report, and every
old/new pixel hash is exactly `a54a357253c49603`. This is a real library/client
fixture, not a copied resource-accounting model.

The complete optimized FreeType/HarfBuzz/Cairo/Pango runtime is rebuilt and its
canonical checker passes. All four archive hashes and source patch hashes are
verified; the only source-patch change is Cairo Xlib cache ownership. With that
runtime, the whole FLTK/vendored-libdecor/client sanitizer lifecycle fixture now
passes native X119 and Wayland9 and exits cleanly on both. This removes the
parent toolkit checkpoint's separate800-byte/two-allocation Xlib source report.
The bitmap libraries remain optimized Release dependencies in these sanitizer
clients; no fully instrumented bitmap-stack claim is made.

Run `scripts/test-cairo-xlib-lifetime.py --prefix PREFIX --reference-prefix OLD
--output OUTPUT` with a private Xvfb available in PATH to retain the old negative
and compare all rendered pixels. No suppression, numerical tolerance change or
runtime fallback is used. Exact commands, source/binary/output hashes and the
fresh runtime manifest are in
[the receipt](validation/CAIRO-XLIB-CACHE-CLEANUP.json).

No system installation, active session restart, physical headset or GPU-device
test occurred. Standalone-XR's separate Mesa112-byte report remains open.
