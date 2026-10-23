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

In XR input mode, the same action captures the current DomeD view as a full SBS
PNG: left eye first, right eye second. The filename ends in `_SBS.png`. It is
also copied to the clipboard. Each eye uses its runtime pose and asymmetric
field of view, at up to 2048×2048 pixels per eye. Flat windows, curved dome
surfaces, stereo application textures, shell layers and cursor layers use the
same geometry and layer order as the native renderer. CPU copies are requested
only for capture; normal DMA-BUF rendering stays enabled.

OpenXR does not supply this application with other applications' independent
layers or runtime passthrough images. Those are not part of the screenshot;
uncovered pixels are black. An active immersive WebXR projection currently
reports an unsupported-capture notification rather than saving an incomplete
image; return to the normal DomeD view to capture it.

PNG encoding, stereo projection and disk writes run on worker threads. Repeated
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
activation, and XR request/result delivery. XR projection tests cover actual
eye separation, SBS texture ordering (including odd widths), cylindrical
surfaces, alpha composition and invalid dimensions/FOVs. The integration XR
endpoint test supplies a deterministic SBS fixture; it does not require or
claim a live headset capture.

The compositor GPU regression `desktop_capture_keeps_rows_columns_and_crop_in_both_presentations`
checks an asymmetric image pixel by pixel, including crop coordinates, while
Normal (native DRM) and Flipped180 (nested) presentation targets are bound.
Run it with both `DOME_RENDERER=gles` and `DOME_RENDERER=vulkan` to cover both
rendering paths without taking control of a physical display.

The multihead GPU tests stitch two independently rendered outputs with unequal
sizes, negative offsets and gaps, then verify every pixel and a PNG crop spanning
both monitors. They also check mirrored outputs, incomplete capture readiness,
layout/size mismatches and the desktop allocation limit.
