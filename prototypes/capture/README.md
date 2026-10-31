# DomeD Capture prototype

An FLTK recorder for one desktop monitor or one XR eye. It writes H.264, H.265,
VP9 or AV1 video to Matroska, with optional Opus audio and VA-API or Intel QSV
hardware encoding. There is no OBS, Qt or GTK UI
dependency. The controls use DomeD appearance, Unicode bitmap fonts, its shared
file picker, GPU widgets and the universal menu. The control window works on
the desktop or as an ordinary spatial DomeD window.

## Capture paths

| Source | Transport and processing |
| --- | --- |
| Desktop monitor | ScreenCast portal consent → authorized PipeWire remote → latest SHM RGB frame → hardware upload → VA-API scaling/RGB conversion → selected hardware video encoder. DomeD currently produces at most 30 fps. |
| DomeD XR, left/right | Read-only geometry/eye poses → same-user leased DMA-BUF surfaces → EGL render of flat/cylindrical panels → DRM PRIME direct mapping → VA-API scaling/RGB conversion → selected hardware video encoder. No CPU pixel readback. |
| OpenXR mirror, left/right | Existing OBSMirror Linux Vulkan producer → private mmap → selected-eye copy only → hardware upload → VA-API → selected video encoder. The other eye is never copied by this reader. |

With **Intel QSV (Quick Sync)** selected, VA-API handles GPU scaling and RGB→NV12
conversion, then the same GPU surfaces map directly into the selected QSV encoder.
This works for all three sources, including direct DRM PRIME XR input, without
a GPU download or CPU colour conversion. A four-surface reverse-mapped QSV pool
supports FFmpeg/oneVPL implementations that cannot import dynamic VA-API pools.

**Video codec** offers H.264, H.265 (HEVC), VP9 and AV1. Entries without a
compiled FFmpeg encoder for the chosen backend are disabled; actual GPU support
is checked when the encoder opens, before creating an output file. Unsupported
combinations report an error and never switch to software automatically. All
four codecs have been encoded and decoded through both QSV and VA-API on the
Arc A770, including both eyes and flat/curved surfaces. Other GPUs may support a
subset; NVENC has no VP9 encoder.

**Audio** defaults to **Off**. **System output** records the default PipeWire
sink's monitor; **Microphone** records the default PipeWire source. Both use
48 kHz stereo Opus at 128 kbit/s. Audio capture begins only after starting a
recording and stops with it. Devices are selected through the existing system
mixer/defaults. This version selects one audio source at a time. FFmpeg must
provide the `libopus` encoder when audio is enabled.

The direct XR path reprojects DomeD's surfaces from the selected runtime eye
pose, using the shared headset viewport. SBS client windows use the matching
half of their texture. It is a surface mirror, not the runtime's final headset
image: passthrough, other applications' projection layers, procedural backdrop,
floor effects, pointer beams, compositor focus/border effects and animated
shell transitions are not reproduced. Scroll viewports crop the content but do
not paint DomeD's scroll buttons. These differences are deliberate prototype
limits. Locked or expired XR sessions expose no capture geometry. Ending XR or
revoking desktop sharing stops capture and finalizes the file.

Desktop sharing always goes through the portal's monitor picker. Direct XR
capture uses DomeD's existing trusted, same-user GPU transport; explicitly
starting a recording selects this source without a second portal dialog.
This is not an API for untrusted/sandboxed recording clients.

## Build and run

Requires the de-xr FLTK 1.4.5 toolkit, FFmpeg 8 development libraries (avcodec,
avformat, avutil, avfilter), PipeWire, GIO, curl, EGL/GLES, GBM, libdrm,
Fontconfig, zlib, nlohmann-json, CMake and a C++17 compiler. Use the matching
FFmpeg headers and shared libraries. Runtime needs the appropriate GPU driver
and access to its render node; no root access is needed.

From the DomeD root:

```sh
cmake -S prototypes/capture -B build/capture -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_PREFIX_PATH=/opt/fltk-vr/toolkit -DBUILD_TESTING=ON
cmake --build build/capture --parallel
build/capture/dome-capture
```

For direct DomeD XR, build/run the matching compositor with the new
`GET /api/capture/xr` endpoint and an XR helper reporting both eye poses. An
already-running older compositor can still provide desktop portal capture.
Do not restart the active desktop to test a build: the integration test below
uses its own private compositor. System installation can take effect on the
next normal login.

CLI examples (output files must not already exist):

```sh
build/capture/dome-capture --record --source desktop \
  --output "$HOME/Videos/Desktop.mkv" --report "$HOME/Videos/Desktop.json"
build/capture/dome-capture --record --source xr --eye right \
  --encoder qsv --codec av1 --audio system \
  --device /dev/dri/renderD129 --width 1920 --fps 60 \
  --output "$HOME/Videos/XR-right.mkv"
build/capture/dome-capture --record --source mirror --eye left --pid 12345 \
  --output "$HOME/Videos/Application-left.mkv"
```

Stop with **Stop and save**, the global Recording menu, Ctrl+`.` or SIGINT/SIGTERM.
Closing the GUI requests a graceful stop and waits for file finalization.
Ctrl+R starts recording; Super+Q quits. `--seconds N` stops after a finite
recording, and `--bitrate N` sets bits per second (default 12 Mbps).
`--codec h264|h265|vp9|av1` and `--audio off|system|microphone` expose the same
choices as the UI. Defaults remain H.264 with audio off.

Automatic encoding chooses VA-API on an accessible selected render node, or
NVENC when no accessible node exists. It never silently falls back to software.
Choose a render node explicitly for multiple GPUs: direct XR requires the same
GPU as DomeD. `DOME_CAPTURE_DEVICE` supplies the default; otherwise the last
accessible sorted render node is selected. Select **Intel QSV (Quick Sync)** in
the UI or pass `--encoder qsv` for Intel QSV; automatic selection continues to
use VA-API. QSV needs an FFmpeg build with the selected QSV encoder, the Intel media driver and
a compatible QSV runtime (oneVPL on Arc). Both QSV and VA-API are validated on
this machine's Arc A770. NVENC accepts CPU input and uses CPU scaling/conversion; it
has not been tested here because there is no NVIDIA GPU. Software encoding is an
explicit option for diagnostics and automated tests. Direct DRM XR capture
requires VA-API or QSV on the compositor's GPU.

The OpenXR source reuses the wire protocol from
`/home/blux/OpenXR-Layer-OBSMirror`, revision
`77de9c0a3a8d9d794f23f1a20b0b76f3ab53a0bd`. The unchanged MIT-licensed IPC files
and their license are in `third_party/obsmirror`. The optimized selected-eye
reader is in `sources.cpp`. The producer layer must be installed/enabled in the
native Vulkan application's environment; this recorder does not install or
enable an implicit layer globally. The producer must use the same
`OPENXR_OBS_MIRROR_DIR`, if overridden. With PID zero the newest active producer
is selected once and pinned for the recording. Producer loss never switches
silently to another application. This stream captures that application's first
projection layer; the separate DomeD source handles DomeD's spatial panels.
The existing producer still reads back both eyes, even though this reader
copies only one. Wine/Proton capture is outside this Linux prototype.

## Performance and reliability

* A separate worker owns capture, conversion and encoding. The UI and headset
  submission never wait for that worker. There is no live preview or extra
  preview copy.
* A four-target GBM pool bounds XR output memory. Imported textures retain their
  producer socket leases until GL work completes. Same-serial textures are
  reused; disappeared surfaces are released. Cross-GPU imports fail explicitly.
* `hwmap` requires a direct mapping, so a driver cannot silently substitute a
  CPU copy. EGL completion currently uses `glFinish` on the recording worker.
  Explicit GPU fence handoff and a DMA-BUF PipeWire producer are future work.
* Desktop dequeue drops older buffers; the XR/OBS sources read current frames.
  Missed deadlines are skipped. Monotonic timestamps preserve real elapsed
  time rather than speeding up footage after dropped frames.
* GPU VPP converts sRGB/full-range input to BT.709/limited-range NV12. Video uses
  no B frames and a two-second GOP. QSV uses an asynchronous depth of one,
  disables lookahead and selects the fast preset to bound latency and retained
  surfaces. VP9 bitstream metadata explicitly carries the colour matrix and
  range so decoders reproduce the same colours as the other formats. Pause,
  streaming, regions/windows, HDR and session restoration are outside this
  prototype.
* Audio has its own PipeWire loop and a bounded 250 ms PCM queue. The recording
  worker owns both codecs and all mux writes. Audio and video use the same
  monotonic epoch; overlapping blocks are trimmed and dropped-sample gaps
  become silence. Audio device/server loss stops and finalizes the recording.
* Format changes stop safely. Outputs use exclusive creation and mode 0600.
  No existing file is overwritten. Encoder errors and trailer/write failures
  are reported, including disk-full failures. A hard crash cannot guarantee a
  final trailer; Matroska is chosen for recoverable packet structure.

`--report NEW.json` includes transport, actual encoder/input format (including
the encoder's `qsv` or `vaapi` surface format), frame
count, dimensions, worker CPU time, average/max encode time, missed deadlines
and errors. These counters describe the recording worker; they do not measure
the audio I/O thread, compositor, producer layer or GPU utilization. Audio sample
and dropped-sample counts are included. Measure headset frame timing
separately before claiming a headset performance result.

## Verification

```sh
ctest --test-dir build/capture --output-on-failure
build/capture/capture-gpu-test /dev/dri/renderD129 artifacts/capture/gpu-check
python3 prototypes/capture/verify-gpu.py artifacts/capture/gpu-check
python3 scripts/test-capture.py
build/capture/capture-gpu-test /dev/dri/renderD129 artifacts/capture/qsv-check qsv
python3 prototypes/capture/verify-gpu.py artifacts/capture/qsv-check
build/capture/capture-gpu-test /dev/dri/renderD129 artifacts/capture/av1-check qsv av1
python3 prototypes/capture/verify-gpu.py artifacts/capture/av1-check av1
python3 scripts/test-capture-audio.py
DOME_CAPTURE_TEST_ENCODER=qsv DOME_CAPTURE_TEST_WIDTH=1920 python3 scripts/test-capture.py
DOME_TEST_CAPTURE_APP="$PWD/build/capture/dome-capture" \
  DOME_CAPTURE_TEST_ENCODER=qsv \
  uv run --no-project --with dbus-next --with python-xlib --with pillow \
  --with PySide6 python scripts/test-screencast.py
```

GPU output directories must be new. `capture-gpu-test` uses actual GPU images,
not mocked GL/VA-API calls; it covers both eyes, cylinder/flat geometry,
orientation, buffer reuse and revocation. The decoder verifies every generated
frame and expected colours. `test-capture.py` adds actual native Wayland SBS
pixels, private DomeD capture API/leases, real hardware encoding and valid
recordings after XR revocation. Tracking is synthetic; no headset session is
opened. The portal test verifies consent/cancellation and decoded desktop
pixels on its own D-Bus, PipeWire graph and display. PySide6 is only a fixture
dependency of the existing portal test, not of this application.
The audio test uses separate known tones in an isolated default output and
microphone, verifies decoded source selection and A/V timing, confirms audio-off
has no audio track, and checks safe finalization after PipeWire server loss.

Live headset/runtime and NVIDIA testing remain outstanding. Keep this labelled
as a prototype until those tests and the missing runtime layers are addressed.
