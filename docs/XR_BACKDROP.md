# Desktop and XR backdrops

Appearance has separate **Desktop backdrop** and **XR backdrop** options.
Desktop backdrop switches between **LinuXR** and **None**; XR backdrop switches
between **L.A.2097 – Precipice** and **None**. Each choice persists independently.

The LinuXR desktop artwork is an original SVG with a path-drawn wordmark,
abstract VR/AR visor, head strap and dome projection arcs. Dark mode uses muted
slate/teal; light mode uses pale blue-grey. Changing the colour scheme updates
it immediately. The compositor rasterises the selected vector only when output
size or theme changes, then reuses a cached GPU texture behind all windows and
shell surfaces. It covers each output independently, stays out of the XR city
scene, and is omitted by the lock shield. None retains the plain theme background.

Both SVG colour variants are installed system-wide in
`/usr/local/share/backgrounds/domed/` and contain no font or image dependencies.

## Precipice XR environment

DomeD can render DEEPFIELD / E’van Johnston’s **L.A.2097 – Precipice**
Vive v0.9.9.4 city as its native stereo session environment. Original release:
<https://deepfield.itch.io/la2097-precipice>.

The scene is centred on the original seated balcony viewing point, aligned to
DomeD’s configured **room anchor X/Y/Z and yaw**. Geometry remains in metres;
each eye uses its actual tracked position, orientation and asymmetric FOV.
The environment stays in room space even when the desktop follows the headset.
Re-anchoring the dome moves the environment with it. No window, taskbar item,
input target, independent OpenXR session or child game process is created.

A single stereo projection layer precedes every desktop layer. Desktop surfaces
therefore remain in front, including curved panels, bars, menus and popups.
The indexed mesh and textures are uploaded once to device-local GPU memory. The
scene uses depth testing, alpha cutouts and additive signage. Traffic, lift and
neon transforms are updated independently from the baked static geometry; rain,
steam and beacon particles use GPU instancing and a shared pausable clock. Its rendering resolution
is capped independently at 1536 pixels on the longest edge of each eye.

## Local import

Download the free **L.A.2097 Vive** ZIP from the original release page. The
importer verifies SHA-256
`3dd6adef06c032cd195434a745790fcb2a589537f8ae330057c099fd3c5b4aeb`
and reads only serialized scene/resource files. It never executes the Windows
player or scripts. Build and import as the regular user:

```sh
# ffmpeg and libzstd are required by the offline baker.
python3 -m venv .local-deps/precipice
.local-deps/precipice/bin/pip install UnityPy==1.25.4 numpy Pillow
.local-deps/precipice/bin/python scripts/import-precipice.py /path/to/LA2097.zip
```

Default destination:
`$XDG_DATA_HOME/domed/environments/la2097-precipice`
(or `~/.local/share/domed/environments/la2097-precipice`). The renderer also
searches `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`).
`DOME_XR_BACKDROP_PATH` explicitly selects an imported directory for testing.
The original artwork remains attributed to its author and is separate from
DomeD’s MIT source. The ZIP and converted artwork are not embedded in Git.

Choose **Appearance → XR backdrop → L.A.2097 – Precipice**
(or **None**). Precipice is selected by default when assets are available. A
missing or invalid import logs a diagnostic and leaves the desktop usable.
Import assets before starting the XR helper. The controls live in Appearance; preferences persist with XR configuration.

The import preserves 1,148 active mesh renderers / 161,992 triangles, transforms,
static-batch subsets, authored material colours, signage textures, emission,
baked ambient/point/spot/directional vertex lighting, and exponential distance fog.
The splash logo and inactive objects are omitted.

## Animation and sound

The native bake includes 40 traffic/lift/rotation tracks, 14 scrolling-texture
scripts and 31 rain/steam/beacon emitters (9,628 instanced particles). Cars follow
the authored route axes and wrap endpoints; lifts use compact curves baked at
60 Hz to remove the original scripts’ dependence on display refresh rate. Neon
rotates about its original pivots. Emitters preserve original positions,
materials, rate/lifetime/size/gravity and sampled velocity parameters. Beacons
use their original ping-pong pulse settings. Native equivalents replace Unity’s
runtime scripts and particle simulation; Unity-specific shading/postprocessing
and shadow maps can still differ.

Six active original audio sources are imported: the VR camera’s city ambience
and positional vehicle loops. The inactive desktop camera’s separate music
source is not included. Original reverse playback and pitch are baked offline;
ambience is stereo and vehicle loops are mono Opus. At runtime, libsndfile
streams bounded blocks at 48 kHz into a native PulseAudio/PipeWire mixer. Vehicle
audio follows the same animation transforms, with distance attenuation and
head-relative stereo panning. No external audio-player process is launched and
long ambience tracks are never loaded into memory in full.

When the backdrop is enabled, **Appearance** shows **Backdrop volume** (0–100%,
5% steps; 0% mutes) and **Backdrop Y rotation** (0–355°, 5° steps). Click the minus
or plus half of a numeric control, or use Left/Right. Rotation affects only the
backdrop and positional sound, preserving the desktop dome’s anchor, orientation,
curvature and window positions. Disabling the backdrop hides the dependent
controls while retaining their values. Reset Appearance restores their defaults.
The normal audio mixer also exposes the **DomeD XR backdrop** stream.
Fullscreen suspension, locking, hidden rendering and stale tracking stop sound
and flush queued audio. Animation and audio resume together without advancing
through the time spent suspended. Losing the audio device leaves rendering usable.

## Size and baking

Positions retain float32 metre precision. Vertex indexing removes triangle-list
duplication; UVs and baked light/emission/alpha use half floats. The 22.25 MiB
expanded mesh becomes about 6.45 MiB of indexed GPU data. Offline zstd compression
reduces mesh/index storage to about 2.0 MiB, decoded once when loading. Particle
spawn data is similarly compressed; textures are content-deduplicated and PNG
optimised losslessly, retaining their original resolution. Solid material colours
and static illumination are baked into shared vertex buffers rather than
requiring separate materials or runtime lights for each object.

The complete import, including animation and Opus audio, is about 11.0 MiB,
compared with about 24.9 MiB for the original static import. Exact sizes are
recorded in the installation manifest. Runtime dependencies are libzstd,
libsndfile and libpulse-simple, loaded from the host; no Unity player is required.

## Fullscreen applications

The renderer submits no backdrop layer and does no scene GPU rendering while:

- a native DomeD XR experience owns the active handoff lease;
- an immersive WebXR session owns the browser bridge, including before its first frame;
- fullscreen XR stereo media is presented;
- the OpenXR runtime reports another main session visible through `XR_EXTX_overlay`;
- DomeD is locked, not visible, tracking is invalid, or the runtime skips rendering.

The imported GPU scene remains cached and its clock/audio stay paused. Once these conditions clear, it resumes
at the same room anchor, without changing desktop geometry, focus, bars or input.
An ordinary maximized lane window is still part of the desktop, not an immersive
fullscreen application. The native handoff flag is exported through
`/api/xr-control`; session tokens remain private.

## Verification

```sh
cargo test --features xr --lib xr_backdrop
DOME_XR_BACKDROP_PATH=/path/to/import cargo test --features xr --bin dome-xr \
  imported_scene_stereo_vulkan -- --ignored --nocapture
```

The Vulkan test renders original geometry into two eye images and reads it back
without opening an XR session. It verifies a nonblank image, stereo differences
and opaque background alpha, checks a pixel-stable paused frame and a changing
resumed frame, and saves `artifacts/precipice/stereo.png` and
`artifacts/precipice/animated-resumed.png`.
The offline audio test decodes and mixes the original clips without opening an
audio sink. Automated tests also cover anchors, route wrapping, pausable clocks,
stale audio input and suspension/restoration policy. Set
`DOME_BACKDROP_BENCH_FRAMES=90` for a GPU submission/completion timing sample.
Headset appearance and independent runtime visibility events need headset QA.
The normal desktop screenshot/export path currently captures desktop surfaces;
it does not include the environment projection layer.
