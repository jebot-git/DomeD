# htop performance floor

The [de-xr htop fork](https://github.com/jebot-git/htop-xr) adds an FLTK/OpenGL
frontend over upstream htop's collectors. Console launches retain the upstream
TUI. On an XR Wayland socket, the process table stays in its window and measured
CPU/RAM/SWAP bars with BDF labels are published as a separate floor canvas.

`performance_floor` accepts private `htop-floor.json` and `htop-floor.rgba` files
in XDG_RUNTIME_DIR. Version 1 JSON supplies a UNIX timestamp, enabled flag,
nonzero serial, 1024×256 dimensions and three ordered CPU/RAM/SWAP ratios. The
pixel file has `HTPF`, the matching little-endian u64 serial, and exactly
1024×256×4 top-down RGBA bytes. Reads are bounded and require mode 0600 regular
files owned by the compositor user; symlinks and FIFOs are rejected. Samples
expire after 2.5 seconds, including after a producer crash. There is no animation
or interpolation of missing measurements.

A separate reserved layer is placed inside the forward edge of STAGE bounds,
with a 4:1 aspect ratio and maximum width of 2.8 metres. Bounds follow cmuxr's
runtime/configured rectangle and `--floor-size` override. It does not invent a
floor from LOCAL space, take input, or replace cmuxr's spectrum. The performance
strip renders above the spectrum, beneath application surfaces. Locked scenes
and fullscreen XR media suppress it.

Build dome-xr with the normal optimized `nested,xr,standalone` feature set.
`cargo test --release --locked --features nested,xr,standalone --lib` covers
validation, expiry, exact serial/frame matching, private reads and placement.
The initial implementation passed 104 library tests and submitted 120 real
performance-floor frames through WiVRn on a Meta Quest Pro, with a clean exit.
Native FLTK/Wayland captures and bridge tests live in the htop fork.

Install the prepared renderer for future XR launches; do not restart the active
desktop or display-manager session to apply this change. `dome-xr --report FILE`
includes `performance_floor_frames` for runtime verification.
