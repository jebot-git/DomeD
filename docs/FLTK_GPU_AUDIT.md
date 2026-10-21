# de-xr FLTK and GPU audit — 2026-10-21

Scope: all 13 application repositories in `/home/blux/de-xr`, plus the `fltk-vr`
source collection. Vendored copies are not counted as separate applications.
Source inspection distinguishes rendering an application, composing its window,
and decoding/encoding its media. Linking OpenGL alone does not establish a
GPU-resident pipeline or hardware execution.

## Changes made from this audit

* `DomeD/native/tray`: native FLTK/OpenGL service managers for KDE Connect,
  Fcitx5, KWallet and arbitrary applications exporting StatusNotifier/DBusMenu.
  It exports three native tray items, a universal application menu, shares DomeD
  appearance, preserves existing daemons, and keeps blocking service work off
  the UI/compositor thread. Existing managed tray activation/settings actions
  open this frontend. Qt frontends require the explicit `DOME_TRAY_FRONTEND=qt6`
  session override. Fcitx's duplicate external tray item is suppressed while
  the native input-method item is registered.
* Xwayland-satellite connects on a dedicated desktop bridge socket. Translated
  X11 toplevels start desktop-only even when XR has focus; explicit user transfer
  remains available. This is based on connection identity, not mutable WM_CLASS.
* `fltk-wpe-xr`: native OpenGL content surface imports producer DMA-BUF planes
  directly as EGLImages. Rendering and release fences protect buffer reuse.
  Native EGL browser rendering defaults to `egl-direct`; `shm` and `egl-readback` remain
  explicit compatibility/diagnostic options. FLTK's GLX backend keeps a reported
  compatibility copy, since EGLImage interop across the bundled and host GLX
  drivers is not safe. A direct X11 path needs an EGL-based FLTK X11 driver. Readback is materialized on demand
  for screenshots or a reported import fallback. Reports count CPU readbacks.
* `FLtube`: application windows use the shared OpenGL FLTK widget driver and
  cached text textures rather than CPU-only Fl_Double_Window painting.

## Project assessment

| Project | Default frontend / GPU path | Remaining work for complete FLTK/GPU coverage |
| --- | --- | --- |
| DomeD | FLTK shell policy; GPU Wayland composition, Vulkan through Mesa Zink by default, optional GLES; Vulkan/OpenXR with DMA-BUF transport | Changed shell layers are still Cairo/FreeType CPU raster uploads. A native shell renderer needs a texture/command interface instead of the current CPU pixel ABI. Polkit and daemon-owned wallet authorization/change-password prompts remain toolkit dependencies. Fcitx candidate/preedit panels retain the engine's Classic UI; replacing that UI requires a native Fcitx UI addon, beyond its tray/settings frontend. Theme lookup, text layout, accessibility and full fractional-scale/IME coverage remain incomplete. |
| arc-sbs | FLTK OpenGL controls; Intel OpenVINO inference and OpenCL stereo backend available | Inference outputs and OpenCL results still cross host memory; video decode/encode pipes include CPU frame processing and upload. GPU inference is not the same as an end-to-end zero-copy conversion. AMD/NVIDIA paths and full device/runtime parity remain separate work. |
| cmuxr | FLTK OpenGL controls, spectrum and native StatusNotifier; original audio core | Full real-library/relocation regression remains. CPU audio decode/analysis is not GPU framebuffer readback and has no demonstrated benefit from moving to graphics APIs. |
| fsv | FLTK OpenGL controls and 3D scenes | Standalone controller/hand XR frontend, trash restore/empty, places, drag/drop and full file-operation UI parity remain. Legacy opt-in Qt presentation has a raster readback path; the default FLTK scene avoids it. |
| qimgvr | FLTK OpenGL image canvas and libmpv rendering | RAW, animation, ICC, printing and high-quality scaling parity remain incomplete. Image edits/decode/clipboard operate on CPU pixels. Clipboard/export readback is intentional; it must not be removed at the cost of those features. |
| LightXR | FLTK OpenGL greeter, original LightDM protocol; separate XR presenter | Greeter-to-presenter JSON/pixel transport still needs a DMA-BUF/fd protocol to eliminate its CPU image path while preserving privilege separation and PAM flow. Physical login and headset testing remain necessary. |
| ghostty-xr | Qt-free native Wayland/EGL/SDL/OpenXR frontend, original terminal engine | It is not a FLTK widget frontend. Terminal glyph/row rasterization still originates on the CPU before GL upload. Eliminating that work requires a glyph-atlas renderer, not deleting readbacks. SDL diagnostic presentation is not evidence of CPU-only final composition. |
| mpvr | Qt-free mpv/gpu-next, OpenGL/libplacebo and DomeD SBS surfaces | No conventional FLTK frontend to port. Hardware decode depends on driver/codec selection. Software decode, subtitle rasterization and requested captures remain valid paths; full physical headset/stream validation is outstanding. |
| fltk-wpe-xr | FLTK browser; direct EGLImage/OpenGL content presentation after this patch | Toolbar/standard dialogs remain FLTK raster widgets. Engine feature parity, accessibility and full input/WebXR/device matrix still need testing. GLX/driver fallback and screenshot readback are explicit and counted. The default pinned nightly's sandbox/cookie limitations are independent of FLTK/GPU work. |
| dillo-quickjs-xr | FLTK browser, CPU FLTK document/layout/image painting | A GPU document/viewport drawing backend remains work. JavaScript, layout and image decode are CPU tasks, not readback defects. X11 clients use the desktop-only bridge when not built as native Wayland clients. |
| FLtube | FLTK, now OpenGL application widgets; playback delegates to mpvr | Thumbnail decode and yt-dlp/network work remain CPU. No per-video-frame CPU renderer is needed in the FLTK search application. Full accessibility/IME/fractional-scale regression remains. |
| gmsh | Native FLTK GUI with OpenGL geometry/mesh view; DomeD live SBS mesh surface | Mesh generation/solver work is CPU computation, not rendering readback. Export/capture helpers intentionally read pixels. Native XR mesh transport uses the compositor path; remote controllers and broad mesh/driver testing remain. |
| open-movie-editor | Original FLTK editor, OpenGL preview; QSV codec registry and export | QSV decoded frames are downloaded for the existing CPU timeline/effects; composed frames are uploaded for QSV encode. Removing those transfers requires a GPU timeline/effect graph and hardware-frame interfaces across the editor, not bypassing edits or effects. Full GPU residency is not implemented. |
| fltk-vr collection | FLTK is the default; Qt remains opt-in | Mirror the new DomeD companion and build/stage integration. A snapshot collection is not proof that all upstream feature sets or rendering pipelines have been ported. |

## Why some CPU paths remain

No continuous browser-frame readback is needed for normal direct GPU presentation;
that path has been replaced. Other copies have consumers that require pixels:
image files, clipboard data, screenshots, legacy HTTP frame clients and CPU-only
editor effects. Removing them without replacing their consumer would silently
remove functionality. Compositor screenshot and HTTP fallback capture code is
separate from the normal DMA-BUF/OpenXR transport.

GPU text still uses CPU font shaping/rasterization on cache misses. Cached glyph
textures and GPU composition eliminate repeated drawing transfers; forcing font
parsing, application logic, file operations or audio through Vulkan is not a
useful porting goal.

## Evidence and limits

`artifacts/fltk-gpu-audit/` contains source revisions/dirty-state inventory,
OpenGL/Vulkan device probes and Intel VA-API capabilities. The host reports
accelerated Mesa OpenGL on **Intel Arc A770**, with both that discrete device and
Intel UHD 770 available to Vulkan. llvmpipe is also installed; its presence does
not mean the default session uses it. Production launchers do not force software
rendering. Xvfb fixtures intentionally use software rendering where appropriate.

`artifacts/tray-frontends/` records isolated service, UI and bridge regressions.
Native tray hardware smoke reported `Mesa Intel(R) Arc(tm) A770 Graphics (DG2)`.
The private bus tests use disposable credentials and never enumerate real wallet
entries or change the user's input configuration or device pairing.

The WPE hardware animation probe reported **174 frames, zero CPU readbacks** in
normal direct presentation. This is a functional transport test, not a frame-rate
benchmark. Snapshot and compatibility tests are recorded separately. Physical
headsets, WiVRn streaming, PAM login and every third-party tray application are
not certified by these tests.

Protocol implementation references: [Fcitx Controller source](https://github.com/fcitx/fcitx5/blob/master/src/modules/dbus/dbusmodule.cpp),
[KDE Connect device API](https://github.com/KDE/kdeconnect-kde/blob/master/core/device.h),
[KWallet API](https://github.com/KDE/kwallet/blob/master/src/api/KWallet/kwallet.h),
and the locally installed WPE Platform `WPEBufferDMABuf.h`/`WPEBuffer.h` headers.
