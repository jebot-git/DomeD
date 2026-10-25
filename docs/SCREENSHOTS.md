# Screenshots

Click the camera icon in DomeD's tray, choose **Take Screenshot** from the Dome
menu, or press **PrintScreen**. The binding is editable in Settings; existing
custom bindings and disabled screenshot bindings are preserved when profiles
are migrated.

On the desktop, drag over the region to capture. Dragging in either direction
works. Escape or the right mouse button cancels; an empty region creates no
file. The selection uses frozen compositor images from all active outputs, so
one region can span multiple monitors. Each monitor keeps its desktop position,
including negative offsets; gaps between monitors are opaque black. Dimming and
the selection outline are excluded from the result. Changing the monitor layout
during selection cancels the capture. The desktop canvas is limited to 32
megapixels, including gaps. Desktop capture renders into separate, consistently
oriented buffers, so native DRM and nested desktops produce the same top-down
pixels without changing the displayed outputs.

The resulting PNG is copied to the session clipboard as `image/png` and saved
under **Screenshots** in the XDG Pictures folder. DomeD reads `user-dirs.dirs`
under `XDG_CONFIG_HOME` (normally `~/.config`), including localized Pictures
paths, and falls back to `~/Pictures`. Captures use unique timestamped filenames
and mode 0600. A notification reports the saved path or an error. If saving
fails after encoding, the image is still copied to the clipboard.

In XR input mode, the same action now exports a spatial snapshot as a
self-contained **glTF 2.0 binary (`.glb`)**, with a filename ending in `_XR.glb`.
Flat windows, tessellated curved dome surfaces, popup windows, shell and cursor
layers retain their captured size and placement in metres. The capture uses
the upright head pose as its origin, with Y up and forward along -Z. Textures
are embedded PNGs with unlit materials, so the file needs no sidecar assets or
external lighting. Premultiplied surface pixels are converted to straight alpha
for glTF; surface opacity is retained. CPU copies are requested only for capture;
normal DMA-BUF rendering stays enabled. The model is also copied to the session
clipboard as `model/gltf-binary`. It is no longer an `image/png` clipboard item.

For stereo application textures, the default scene uses the left-eye image and
a second scene preserves the right-eye image on the same surface geometry.
Ordinary surfaces are shared between these scenes. This is a spatial snapshot
of the compositor surfaces; it does not reconstruct objects/depth inside an
application's stereo image. Standard glTF viewers do not automatically choose
different materials for each eye. Captured eye poses and exact asymmetric FOVs
are stored in `extras.DomeD`; two preview cameras use glTF's symmetric
perspective projection. glTF depth/alpha sorting may differ from OpenXR's
explicit composition order, which is recorded per surface in metadata.

Matching compositor and XR helper builds are needed for glTF capture. A new helper talking to an older compositor retains the full SBS
PNG path; a new compositor also accepts an already-running older helper's PNG
reply with its original `_SBS.png` filename and PNG clipboard type. Captures are
never relabelled across formats. Export is limited to 256 surfaces, 32 megapixels
of source textures in total and a 128 MiB GLB.

OpenXR does not supply this application with other applications' independent
layers or runtime passthrough images. Those are not part of the screenshot;
the model contains only DomeD's own surfaces. An active immersive WebXR projection currently
reports an unsupported-capture notification rather than saving an incomplete
image; return to the normal DomeD view to capture it.

Texture encoding, mesh export and disk writes run on worker threads. Repeated
requests while a capture is pending do not start overlapping jobs. XR requests
time out with an error notification if the native renderer does not answer.

## Verification

```sh
DOME_FLTK_PREFIX=/opt/fltk-vr/toolkit cargo test --features multi-gpu,xr --lib --bin dome-compositor
uv run --no-project --with python-xlib --with pillow python scripts/test-screenshots.py
DOME_SCREENSHOT_RENDERER=vulkan uv run --no-project --with python-xlib --with pillow python scripts/test-screenshots.py
```

The integration script uses private Xvfb, Wayland and D-Bus sessions, a private
home/configuration and a localized Pictures path. It checks exact captured
pixels, reverse dragging, clipboard equality, cancellation, filenames, tray
activation, GLB request/result delivery and model clipboard equality, malformed
or stale replies, and legacy PNG interoperability. XR projection tests cover actual
eye separation, SBS texture ordering (including odd widths), cylindrical
surfaces, alpha composition and invalid dimensions/FOVs. The integration XR
endpoint test supplies deterministic fixtures; it does not require or claim a
live headset capture. Set `DOME_SCREENSHOT_GLTF_FIXTURE` to a file generated by
the exporter to check delivery of that exact model:

```sh
DOME_FLTK_PREFIX=/opt/fltk-vr/toolkit cargo run --example xr-capture-fixture -- /tmp/dome-capture.glb
DOME_SCREENSHOT_GLTF_FIXTURE=/tmp/dome-capture.glb python scripts/test-screenshots.py
```

The spatial export tests check head-relative flat/curved geometry, inward-facing
triangle winding and unit normals, exact eye separation/FOV metadata, left/right
texture preservation (including odd widths), alpha conversion and bounds. The
XR worker test exercises on-demand CPU readback while the rendering source uses
the GPU path, then checks that the result is posted as glTF.

A live DomeD/WiVRn capture was also saved, validated and inspected in the XR
model viewer. The file layout and materials follow the [glTF 2.0 specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)
and [KHR_materials_unlit](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_unlit).

The compositor GPU regression `desktop_capture_keeps_rows_columns_and_crop_in_both_presentations`
checks an asymmetric image pixel by pixel, including crop coordinates, while
Normal (native DRM) and Flipped180 (nested) presentation targets are bound.
Run it with both `DOME_RENDERER=gles` and `DOME_RENDERER=vulkan` to cover both
rendering paths without taking control of a physical display.

The multihead GPU tests stitch two independently rendered outputs with unequal
sizes, negative offsets and gaps, then verify every pixel and a PNG crop spanning
both monitors. They also check mirrored outputs, incomplete capture readiness,
layout/size mismatches and the desktop allocation limit.
