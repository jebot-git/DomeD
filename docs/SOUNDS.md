# Notification and system sounds

DomeD plays short themeable event sounds for new desktop notifications,
critical notices, native Wayland system bells, screen locking/unlocking and
new shell errors. The same notification server and settings apply in desktop
and XR sessions.

Open **Settings** with **Super+,** and set **Notification/system sounds** to
**Off** to disable these cues. Click the row or select it with the arrow keys
and press Return. The change applies immediately and is saved privately in
`$XDG_CONFIG_HOME/dome/sounds.json` (normally `~/.config/dome/sounds.json`):

```json
{"enabled": false}
```

Missing preferences default to enabled. Invalid preferences remain intact and
start with event sounds disabled. This control manages session event feedback;
application media volume and XR backdrop ambience keep their existing controls.

| Event | Theme sound |
| --- | --- |
| New notification | `message-new-instant` |
| Critical notification | `dialog-warning` |
| Native Wayland bell | `bell-window-system` |
| Lock / unlock | `desktop-screen-lock` / `desktop-screen-unlock` |
| Shell error | `dialog-error` |

Minimal themes without lock/unlock sounds fall back to `dialog-information`.
Playback uses libcanberra and the session audio server, including PipeWire's
PulseAudio compatibility service. Install the libcanberra runtime with its
PulseAudio backend and a sound theme, such as the freedesktop theme. No GTK
frontend or player subprocess is launched. Missing audio support leaves the
desktop and notifications usable.

Notifications honor the standard [`suppress-sound` and `sound-name`
hints](https://specifications.freedesktop.org/notification/latest/hints.html).
Suppression takes precedence. Named hints select bounded theme identifiers;
`sound-file` is not implemented. Replacements and already-forwarded events stay
quiet. Brief bursts are coalesced, queues are bounded, and old requests are
invalidated when muting or reenabling sounds. Muting also cancels the current
event sound. Playback and sound-driver initialization run on a separate worker,
without blocking the compositor's input/render loop. The backend uses the
[native libcanberra event API](https://0pointer.de/lennart/projects/libcanberra/gtkdoc/libcanberra-canberra.html).

## Validation

```sh
cargo test --release --locked --offline --features standalone,multi-gpu --lib
cargo test --release --locked --offline --features standalone,multi-gpu --bin dome-compositor
uv run --no-project --with python-xlib --with pillow --with dbus-next python scripts/test-system-sounds.py \
  --binary /absolute/path/to/dome-compositor --output /new/private/control-results
uv run --no-project --with dbus-next python scripts/test-system-sounds-audio.py \
  --binary /absolute/path/to/dome-compositor --output /new/private/audio-results
```

The control test uses temporary profiles, a private bus and display, and a
libcanberra fixture that records requests without audio. It checks normal,
critical, named and suppressed notices, quiet replacements, native desktop and
XR settings, Wayland bells, persistence and unavailable-backend behavior.
A deliberately delayed backend also checks that rapid Off/On changes cancel
in-flight audio without replaying an older notice.
The audio test decodes a real event through libcanberra into a private
PipeWire/PulseAudio virtual sink and measures nonzero PCM after a silent
baseline. It does not open physical audio devices or connect to the host graph.

Installing a new compositor does not restart the running desktop. These
features become available when the next DomeD compositor starts.
