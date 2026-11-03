# Cairo Xlib deferred SHM lifetime

The canonical Cairo patch now releases deferred SHM allocation records before
destroying their priority queue. Surface finish can enqueue these records while
server requests remain in flight; the queue destructor frees its pointer array
but does not free the records. The existing pool destructor still owns the shared
memory. The correction frees only record metadata and adds no synchronization,
pixel conversion, public API or font-policy change.

The regression uses the actual public similar-image API and verifies device-owned
SHM, independent Xlib pixels, fresh server use after the read, repeated device
finish and display close. With leak detection enabled, the old SDK leaks128 bytes
in four records; the new SDK exits0 without a report. All four independent pixel
hashes are identical. The existing four cached-source display-lifetime checks
also pass. No leak suppression or software-format fallback is used.

A fresh regular-user optimized FT/HB/Cairo/Pango SDK builds with the canonical
script. Its153 public headers exactly match the preceding SDK; explicit staged
runtime/manifest validation passes. Both SDK prefixes are retained independently.

The actual owned-sanitizer Dome Sculpt package Open/Save chooser flow previously
passed seven operations then failed clean exit with one32-byte SHM allocation.
The same application and FLTK binaries pass all eight checks and clean exit with
the new bitmap runtime. This is a runtime-library replacement proof, not a fresh
FLTK/frontend build against the new prefix. Native source/project/package bytes
remain exact; no physical headset or system session was touched.

The first old probe encountered Xvfb's last-client reset before its fourth
connection, retaining three pixel checks and a96-byte leak. The final private
server uses `-noreset` and completes four connections in both variants. A missing
Xvfb PATH setup and a deliberate manifest refusal caused by loading the installed
stack are preserved separately. Corrected staged-library selection passes.

See `shm-lifetime-proof.json` for source, commands, artifacts and limitations.
Dome Sculpt still needs its manifest guard/coherent SDK update. The independent
standalone-XR Mesa112-byte report remains unresolved. No system installation or
active-session restart occurred.
