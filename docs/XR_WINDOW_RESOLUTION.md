# New XR window resolution

DomeD Settings provides **New XR window resolution** with these content-size
presets, independent of the headset model, monitor size and UI scale:

| Preset | Main window | Popup/dialog budget |
| --- | --- | --- |
| 720p | 1280 × 720 | 640 × 360 |
| 900p | 1600 × 900 | 800 × 450 |
| 1080p (default) | 1920 × 1080 | 960 × 540 |
| QHD | 2560 × 1440 | 1280 × 720 |

The preference is stored as `xr_window_resolution` in the existing
`$XDG_CONFIG_HOME/dome/xr.json` configuration (`~/.config/dome/xr.json` by
default). Older configurations default to 1080p. Changing this option affects
new windows and parented dialogs launched through the XR socket. It does not resize
existing main windows, change UI/buffer scale, change `xr_render_scale`, maximize a
window, or modify its `xr_scale` spatial zoom. Later user resizing remains
available, including below the launch preset.

Parented XR dialogs default to half the configured width and height, including
nested dialogs; this is a quarter of the main window's pixel area. Smaller menus
retain their natural size. Popups with client-controlled buffers (menus, input
methods and fixed dialogs) also fit within this budget when displayed in XR,
using uniform scaling that preserves their complete content and input mapping.
Explicit client minimum/maximum sizes remain authoritative for buffer sizes,
so a fixed 320×180 dialog stays 320×180. Desktop launches, notification surfaces
and X11 bridge windows do not receive the launch-size override. Restored or explicitly
assigned geometry takes precedence over the default 720×460 launch rectangle.
The first configure waits for the initial surface commit so size constraints
are available; decoration negotiation must not send it early. If a toolkit
announces a transient parent after that configure, DomeD corrects its own launch
suggestion once. Later client/user resizing remains authoritative.

Content resolution is separate from XR capture density. The latter still
multiplies logical pixels and remains subject to the 4096-pixel capture dimension
cap. Increasing capture density alone does not guarantee additional client text
detail when the toolkit does not honor the preferred buffer scale. This option
instead gives the application more content pixels without changing its UI scale.

## VRCX comparison, 2026-10-25

The running `com.vrcx-0.app` window (id 31) measured 1272×950 logical content pixels
in both XR and desktop presentation. Its configured XR density remained 200%,
and the GPU capture was 2544×1900 in both modes. The original presentation was
restored after this brief presentation-mode comparison. These capture dimensions
are not a measurement of the application's internal rasterization density.
VRCX's source constructor requests 1360×800; DomeD's historical ordinary launch
rectangle is 720×460.

An isolated compositor test with the actual installed VRCX executable, an empty
profile, offline mode and preview-read-only mode verified a 720×460 desktop
launch and a 2560×1440 XR launch with QHD selected. A native FLTK probe verified
all four presets with identical UI scale 1 and buffer scale 1; its small modal dialog
stayed 320×180, resizable/nested dialogs used half resolution, and a fixed-size
client stayed 640×480. Changing presets did not
resize an already-open window. Unit/layout tests cover setting adjustment,
configuration round trips and independence from render density and dome geometry.

New compositor builds take effect in a new DomeD session. Installation preserves
the active compositor and applications.

### Popup placement in XR

DomeD applies one placement rule to application popups and parented dialogs,
including FLTK menus, choices, file choosers, and nested dialogs. Their center
starts at the parent center, or an explicit context-click origin, with a small
5° downward offset and a 14 cm forward separation. They no longer float wholly
above the parent. The origin beam continues to identify the owning control.

The renderer and pointer use the same fitted geometry. When the headset reports
its binocular viewport, the whole popup is translated into the comfortable
central view, with uniform shrinking only when needed to fit its corners.
When a parent is outside view, the relocated dialog faces the headset and
retains its viewing distance. This also handles narrow/asymmetric FOVs, tilted
parents, and nested popups.
The half-resolution budget, surface UVs, and native client input coordinates
continue to apply. Desktop popup positioning follows the normal client layout.

The desktop advertises its popup placement capability to the XR helper. A helper
restarted during a continuing older desktop session retains that desktop's
placement until the next normal session start, keeping visible surfaces and
pointer hits synchronized across a staged update.
