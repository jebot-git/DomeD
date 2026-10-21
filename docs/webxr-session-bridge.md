# WebXR through Dome's existing XR session — protocol v1

The browser uses a private OpenXR **client facade**, not the WiVRn runtime. Only
`dome-xr` creates a real OpenXR instance/session and submits layers to WiVRn.
It composes browser projection content before the desktop/shell layers in that
same session. Browser consent remains mandatory before starting a WebXR session.

## Transport

Unix stream socket `$XDG_RUNTIME_DIR/dome-webxr-PORT.sock`, where PORT is the
Dome control server port. `DOME_WEBXR_SOCKET` overrides the browser's path.
The socket is mode 0600 and both ends verify peer UID using SO_PEERCRED.
Each RPC uses a **new connection**, one request and one response. The process
opens a lease with `begin_session`; an unpredictable token identifies its
subsequent requests. One immersive browser session is allowed at a time.
Every request begins with a 4-byte little-endian JSON byte length (1..65536),
then UTF-8 JSON. Current `end_frame` requests send the declared RGBA bytes after
the JSON. A future advertised DMA-BUF transport would send an SCM_RIGHTS FD
with one extra byte instead; the current host rejects that transport.
Every response is u32LE length + JSON, without ancillary data. Errors are
`{"ok":false,"error":"reason"}`. Success always includes `"ok":true`.

All poses are right-handed metres, with `position:[x,y,z]` and
`orientation:[x,y,z,w]`. FOV objects use radians and keys `left,right,up,down`.
Rects are integer `{x,y,width,height}`. Validate dimensions, array lengths,
finite numbers, pose normalization and bounds before allocating or importing.

## Requests

- `{"op":"hello","version":1}` returns
  `{"ok":true,"version":1,"name":"DomeD XR session","width":1280,"height":720,
  "view_sizes":[[640,720],[640,720]],"max_width":4096,"max_height":4096,
  "blend_modes":["opaque"],"stage":true,"period":13888888}`.
  Actual sizes and period come from the running Dome session. No tracking is
  returned before a lease is granted.
- `{"op":"begin_session"}` returns `{"ok":true,"token":"..."}` or busy.
  The token is bound to the UID/PID, with an inactivity timeout of five seconds.
- `{"op":"wait_frame","token":"...","after":123}` waits for a newer host
  frame (at most one second), returning
  `{"ok":true,"frame_id":124,"time":123456789,"period":13888888,
  "should_render":true,"visible":true,"focused":true,"head_flags":15,"view_flags":15,"head":POSE,
  "views":[{"pose":POSE,"fov":FOV},{"pose":POSE,"fov":FOV}]}`.
  Time is the actual host OpenXR predicted display time, in OpenXR nanoseconds.
  VIEW/LOCAL/STAGE transformations must use these poses; no fabricated head pose.
  Wire poses use the host's actual STAGE-or-LOCAL origin. The client anchors
  LOCAL at the actual initial head position and gravity-aligned yaw after
  consent, and transforms authored poses back into the host basis on submission.
  STAGE is advertised only when the real runtime provides it.
- `{"op":"end_frame","token":"...","frame_id":124,"width":1280,"height":720,
  "transport":"rgba","layer_flags":2,"bytes":3686400,"views":[{"rect":RECT,"pose":POSE,"fov":FOV},
  {"rect":RECT,"pose":POSE,"fov":FOV}]}` is followed by tightly packed RGBA8.
  Pixels are top-to-bottom with linear RGB, matching the facade's GL_RGBA8
  swapchain. Dome uploads them into a linear UNORM swapchain. The proposed
  `transport:"dmabuf"` instead includes
  `"dmabuf":{"device":DEV,"fourcc":FOURCC,"modifier":MOD,"stride":STRIDE,
  "offset":OFFSET,"y_inverted":false}` and is followed by a byte with exactly
  one SCM_RIGHTS FD. The sender completes producer rendering before sending.
  The response is delayed until Dome has copied/sampled the frame into its own
  real OpenXR swapchain and GPU work has completed, making buffer reuse safe.
  No layers: `{"op":"end_frame","token":"...","frame_id":124,"empty":true}`.
- `{"op":"end_session","token":"..."}` removes browser content and releases
  the lease. It never requests exit of Dome's real OpenXR session.

The browser shim retains its frame timing/poses between xrWaitFrame and
xrLocateViews; xrEndFrame passes the authored projection poses/FOVs. The host
uses those poses even if it displays the submitted image on the next frame.
It drops content older than 500ms rather than leaving a stale scene visible.
The current host advertises only the `rgba` transport; DMA-BUF is reserved for
a later implementation. The first implementation supports one RGBA projection swapchain; unsupported
composition types, depth, action inputs, hands and layers return honest errors
or are not advertised. Browser Exit XR and navigation close only this lease.

The optional bridge must not interrupt Dome's desktop presentation. Bind failures
are logged and leave the existing XR session running. `--music-floor-only` does
not expose a browser endpoint. Missing linear RGBA8 support rejects browser
uploads, while desktop layers keep their existing format.
