# Vertical view field and XR render density

Evaluation of the current implementation (2026-10-25).

## Vertical view field

`dome_vertical_span_deg` sets the physical dome lane extent; lane height and
spacing are derived from it (`Config::lane_extent` / `lane_spans`).
`vertical_fov_deg` does not change that extent or the OpenXR eye projection.
The runtime supplies the actual optical field of view.

The separate setting currently controls view-relative UI sizing and fallback
limits: context-panel fitting, expanded settings/pullout panel sizing, the
mouse viewport when no runtime viewport is available, and fullscreen stereo
media when no valid binocular viewport is available. Runtime viewport data
already takes precedence for mouse confinement and fullscreen media.
See `src/context_menu.rs`, `src/xr_input.rs`, `src/xr_input/media_view.rs`, and
`src/bin/dome-compositor/xr_bridge.rs`.

Recommendation: keep the stored value for compatibility and fallback behavior,
but describe it as **UI vertical view span**, preferably in advanced settings.
It is useful for comfortable head-relative panel sizing, independently of the
room dome's height. Calling it headset FOV is misleading. A future separation
of panel comfort span from runtime-derived viewport limits would be clearer;
simply removing it or tying it to dome height changes existing popup behavior.

## XR render density

`xr_render_scale` (1–2, default 1.5) is a source-window texture density, not an
OpenXR eye-buffer scale. In XR, the compositor advertises it as the preferred
Wayland fractional scale (and rounded-up integer scale) to clients. It also
scales offscreen window/popup capture dimensions, subject to the texture-size
cap. Clients that honor the preference can rasterize text and vectors at the
higher density. Clients that ignore it only have their existing pixels sampled
into a larger capture; that cannot recover missing detail.
See `handlers/mod.rs::update_surface_density` and `xr_gpu::capture_size`.

WiVRn/runtime supersampling affects the downstream eye image. Foveation changes
its spatial sampling and/or encoding allocation. Neither can reconstruct detail
absent from the source application buffer. Conversely, more source pixels may
be wasted when eye resolution, angular window size, foveation or video encoding
is the bottleneck. At 2x each dimension, a window capture has four times the
pixels of 1x, before dimension caps; this can increase GPU/memory/transport cost.

Recommendation: retain this independent control, label it **Window texture
density**, and expose it as an advanced quality/performance setting. Start at
the existing 150% default; compare 100%, 150% and 200% on small text at the same
angular window size with WiVRn settings held fixed. Keep the lowest density
that produces a visible benefit without harming frame time. Automatic tuning
would need source-buffer scale, angular pixel density, runtime eye resolution,
and frame-time measurements; WiVRn supersampling alone is not a substitute.

This is a source-code evaluation, not a headset quality/performance measurement.
No settings, defaults, saved configuration keys or optical projections were
changed as part of the evaluation.
