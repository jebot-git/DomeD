# Network, backups and audio mixer

Network and Backups are native FLTK pages of the permanent tray applications
manager. Open them from its menu, use **View → Network / Backups**, press
**Alt+6 / Alt+7**, or run `dome-tray --network` / `dome-tray --backup`.
Additional invocations open the existing instance. Both entries default to
**inside the manager**. **Move to tray** gives each its own StatusNotifier icon;
**Move to manager** collects it again without starting another process.
Closing the window leaves the service running. The same GPU window, live DomeD
palette/fonts, universal menu and Wayland-first FLTK backend serve both pages.
Their compatibility desktop launchers are hidden from the application menu.

## Network

The page uses libnm and the system NetworkManager service. It displays network
devices, connection state, IP addresses, DNS servers, Wi-Fi signal/security,
saved connections and VPNs. Status refreshes while the page is visible. Opening
it does not enable networking, switch radios or scan. Select an adapter and
press **Scan** explicitly. Device removal and service failures appear in the
status area; **Refresh** retries after the service returns.

Select a Wi-Fi network and **Connect…**. New open, WPA2 Personal and WPA3 Personal
networks are supported, including hidden SSIDs. The password input is masked;
an empty password uses a compatible saved profile when one exists. NetworkManager
stores new credentials and restricts new profiles to the current user. The
frontend does not put credentials in its own configuration, logs or clipboard.
**Add wired…** creates and activates an Ethernet profile using automatic IPv4/IPv6.

Saved profiles can be activated, disconnected, renamed, removed, and given an
autoconnect preference. **Edit… → Change IP settings** enables IPv4/IPv6 method,
address/prefix, gateway, DNS and automatic-DNS override fields. Unedited settings
are retained; changing the first address replaces the address list, while an
unchanged address preserves additional addresses. Routes are preserved. Saved
IP edits take effect on the next activation. Removal and disabling all networking
ask for confirmation. Policy authorization follows NetworkManager and the
session's Polkit agent; denied requests are reported in the page.

**Import VPN…** imports an OpenVPN or WireGuard file through `nmcli`; this also
requires the corresponding installed NetworkManager plugin where applicable.
Imported and existing VPN profiles use the same activation controls. There is
no general secret agent or enterprise/VPN credential editor: profiles requiring
interactive secrets, certificates or external authentication must already be
configured with their secrets available. Import/activation failures are shown
instead of launching another graphical frontend. Captive-portal login, hotspot
creation and proxy configuration are outside this implementation.

## Backups

Choose separate source and existing destination directories. **Preview** runs
rsync with `--dry-run`; **Back up now** copies directory contents, including
hidden files and symlinks. Files already present only at the destination are
kept by default. Exclude patterns are one rsync pattern per line. Optional
checksum comparison and compression are available. Commands use argument
arrays and protected rsync arguments, without interpreting path text as shell
commands. Transfer progress and failures appear in the page; **Cancel job**
stops rsync and its child processes.

Changed destination files are retained under
`.de-xr-history/RUN-IDENTIFIER/`. **Mirror deletions** also moves destination-only
files into this history. It requires a successful preview of the exact current
settings and a confirmation before each interactive run. History is excluded
from copying and deletion. It stores replaced/deleted files, not complete
point-in-time snapshots; no automatic history pruning is performed.

**Restore…** copies the latest backup into a chosen existing local directory.
Existing changed files in that restore directory are also retained in its
`.de-xr-history`. Restore does not delete unrelated files. Earlier individual
versions can be recovered from history with the file manager.

One side may be SSH remote, written as `user@host:/absolute/directory` (including
bracketed IPv6 hosts). SSH uses existing keys/configuration, batch authentication
and strict known-host verification. Configure and verify the SSH connection
before using the page; interactive SSH passwords and first-use host-key approval
are not implemented here. Both sides cannot be remote.

**Save profile** writes owner-only JSON under
`$XDG_CONFIG_HOME/dome/backups/UUID.json` (normally `~/.config/dome/backups`).
Local destinations retain their canonical path and filesystem identity. A job
fails if the destination mount has changed; explicitly choose the intended
destination and save again to accept a different disk. This also applies to
manual jobs after loading a saved profile. Remote destination identity is
checked by SSH, not by a stored filesystem identifier.

Choose **Daily** or **Weekly**, then **Apply saved schedule** to save the profile
and enable a per-user systemd timer. The installed `dome-backup-job UUID` helper
runs it without opening a window. Timers use a five-minute randomized delay and
catch up missed runs when the user service manager is available. Jobs run as
the user, with one job at a time per profile. Scheduled backups always preserve
destination-only files; mirroring is rejected by both scheduler and helper.
Choose **Off → Apply saved schedule** to disable the timer, including when the
source or backup disk is unavailable. Inspect job history with
`journalctl --user -u dome-backup-UUID.service`.

## Audio mixer

Open the speaker widget and select its second page, **Mixer**. **Volume** retains
the default speaker/microphone controls and default-device list. Tab or
Page Up/Page Down switches pages in desktop and XR.

The mixer lists output/input devices, playback/recording applications and device
profiles from the local PipeWire PulseAudio compatibility service. Select a row
with the pointer or Up/Down. Left/Right and the slider adjust volume; Enter/Space
or **Mute** toggles mute. **C** and the channel arrows choose all channels or one
channel. All-channel changes preserve relative channel balance. **R** and the
route arrows choose an application device, a hardware port or a device profile,
depending on the selected row. Volume changes are capped at 100%.

Mixer commands revalidate stream/device identities and routing destinations
before changing them. Removed objects and service errors do not block the shell.
Mixer polling stops when the page closes. Default-device policy and persistence
remain with WirePlumber; application routing/profile availability follows the
audio server. The mixer requires `pactl` with JSON output and `pipewire-pulse`;
the first page continues to use `pw-dump` and `wpctl`.

## Build and validation

Build optimized artifacts as the regular user with `scripts/build-tray.sh` and
the compositor's release build. Development requirements add libnm headers to
the shared FLTK/OpenGL/GIO dependencies. Runtime requirements are NetworkManager,
rsync with `--secluded-args` support, SSH for remote backups, user systemd for
scheduling, and the audio tools above. Stage with `scripts/stage-tray.sh`; the
stage/install scripts include the helper, hidden launchers, icons and this guide.
Installation does not restart an active desktop or display manager.

```sh
ctest --test-dir build/tray --output-on-failure
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next \
  python tests/tray/test_network.py
TRAY_TEST_PRIVATE_BUS=1 dbus-run-session -- uv run --no-project --with dbus-next \
  --with python-xlib --with pillow python tests/tray/test_ui.py
DOME_COMPOSITOR_BIN="$PWD/target/release/dome-compositor" uv run --no-project \
  --with python-xlib --with pillow python scripts/test-audio.py
```

The NetworkManager protocol fixture tests real libnm discovery, Unicode SSIDs,
IP/DNS, radio/scan controls, activation, profile editing/removal, permission
errors, hotplug and service recovery. Rsync tests use temporary directories and
real rsync for preview, transfer, hidden files/symlinks, changed-file history,
mirroring, permissions, filesystem identity, locking and cancellation; timer
generation uses a private `systemctl` fixture. Native UI checks include network
controls and a real backup. Audio integration uses private PipeWire/PulseAudio,
policy-only WirePlumber, virtual devices, streams, D-Bus and Xvfb. These tests do
not change host networking/audio, enable real timers, or establish physical
network-device or headset compatibility.

Protocol references: [NetworkManager API](https://networkmanager.dev/docs/api/latest/spec.html),
[NetworkManager profile-update secret handling](https://github.com/NetworkManager/NetworkManager/blob/1.56.1/src/core/settings/nm-settings-connection.c),
and [rsync manual](https://download.samba.org/pub/rsync/rsync.1).
