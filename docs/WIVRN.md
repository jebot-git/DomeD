# WiVRn-Pro / WayVR integration

The native compositor exports `org.domed.Compositor` at
`/org/domed/Compositor` on the user's session bus. An older owner without this
API is never replaced by WiVRn's watchdog. All methods queue work for the seat
thread; the D-Bus thread does not manipulate the renderer or Wayland seat.

* `GetIntegrationState() -> s`: JSON `version:1`, `ready`, `backend` (`drm` or
  `nested`), `xr_active`, `shown`, `notifications` (`domed` when this compositor
  owns the desktop notification service, otherwise `host`), `pointer_protocol:1`.
* `Activate(s) -> b`: allowlisted `toggle_desktop`, `launcher`, `screenshot`,
  `terminal`, or `stop`. The presentation toggle includes both bars and the XR
  desktop, preserving the normal fade/slide and input realm handoff. Screenshot
  requests target XR even when invoked from the watch. Terminal toggles the
  selected existing overlay without adding a tab, and cold-starts it if needed.
* `ForwardNotification(s id, s title, s body, i timeout_ms) -> b`: bounded queue
  into the compositor's common notification stack/history. IDs are at most 256
  bytes, text at most 60 KB, timeout 100–120000 ms. Repeating an ID does not
  redisplay a dismissed notice. Equivalent standard `Notify` calls from WayVR
  use the `x-dome-event-id` hint and share the same bounded 512-ID dedup cache.
* `SubmitPointers(s) -> s`: bounded JSON frame; reply has `version`, last
  processed `sequence`, and two nullable `hits` distances. Feedback is asynchronous.

Pointer frame:

```json
{"version":1,"sequence":1,"time_ns":123456789,"rays":[
  {"tracked":true,"origin":[0,0,0],"direction":[0,0,-1],"buttons":0,
   "scroll":[0,0],"max_distance":100},
  {"tracked":false,"origin":[0,0,0],"direction":[0,0,-1],"buttons":0,
   "scroll":[0,0],"max_distance":100}]}
```

`time_ns` uses CLOCK_MONOTONIC. Frames older than 150 ms, more than 50 ms into the
future, nonfinite values, invalid directions, unknown fields or button bits, and
oversized payloads are rejected. Sequence increases within a D-Bus sender's
stream. Rays are metres in the sender's head coordinates; DomeD transforms them
using its current XR head pose. `max_distance` is the nearest local WayVR overlay
hit (100 m when absent). Button bits 1/2/4 mean left/middle/right; scroll is
horizontal/vertical. A single hand owns the mouse seat while held. A different
sender cannot take a held lease. The lease is 200 ms, physical mouse priority
lasts 350 ms after activity, and neutral is required on acquisition/reacquisition.

DomeD writes `dome/explicit-stop` in XDG_RUNTIME_DIR on an explicit quit, logout,
or termination signal; a successful manual launch clears it. The WiVRn recovery
helper and DomeD login wrapper honor it. See WiVRn-Pro `fork/DOMED.md` for service
installation and resume behavior. Native recovery uses a user-owned singleton
lock acquired before initializing graphics/DRM. Neither tests nor integration
change the user's global OpenXR runtime.

## Verification

```sh
DOME_FLTK_PREFIX=/opt/fltk-vr/toolkit cargo test --locked --features multi-gpu
DOME_FLTK_PREFIX=/opt/fltk-vr/toolkit cargo build --locked --features multi-gpu --bin dome-compositor
python3 scripts/test-wivrn.py
python3 scripts/test-wivrn-session.py
```

The first Python test creates a private D-Bus session, Xvfb display, compositor
and Qt Wayland fixture. A simulated XR heartbeat drives the real raycast and
Wayland pointer paths: button mapping, held acquisition, expiry, tracking loss,
local overlay occlusion, control shortcuts, notification deduplication and
explicit stop. No physical headset is needed. The second tests native-login
crash recovery and termination during backoff using mocked processes/systemd.
WiVRn's offline suite includes its asynchronous supervisor, WVJ3 hand policy and
D-pad compatibility; WayVR's `domed_link::tests` cover queue edges and overflow.
Physical headset alignment, ergonomics and motion latency still need headset QA.

Local validation on 2026-10-26: the complete multi-gpu Cargo suite, private
compositor/Wayland pointer test and all four login recovery cases passed. The
FLTK WiVRn-Pro dashboard also passed its native DomeD Wayland/tray integration
tests. No live compositor, headset or system runtime was restarted.
