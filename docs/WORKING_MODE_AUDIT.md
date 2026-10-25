# Graphical application working-mode audit

Audited 2026-10-24 against the installed FLTK desktop entries and their source
launch paths. A running or focused OpenXR session indicates availability; it
must not select XR output for an ordinary link while DomeD's input realm is
`desktop`.

`dome-link` validates the caller's Wayland peer and private session record,
queries that compositor's current input realm, and selects its matching socket.
It removes inherited `WAYLAND_SOCKET` only after finding a verified destination.
Outside a verified DomeD session it preserves the environment. URI arguments
remain separate arguments, including spaces, Unicode and shell metacharacters.

| Tool or frontend | Finding and resulting behavior |
| --- | --- |
| Dome Browser | Its global D-Bus instance can retain an XR-origin connection. The installed wrapper and desktop handler first present the existing browser window in the requested realm, then retain the browser's existing URL-forwarding/profile logic. |
| mpvr | Automatic playback previously selected DomeD XR whenever the headset was active. It now queries the working realm and selects the desktop socket for desktop playback. The default DomeD MIME entry no longer forces `--domed`. Explicit `--domed` and `--xr-output` choices remain available. |
| qimgvr and Arc SBS | Their shared image worker previously treated XR availability as an automatic presentation request. Automatic connection detection now also requires XR working mode. Explicit presenter commands remain exempt from ordinary launcher routing. |
| Dome Torrent | The native FLTK client accepts torrent files and magnets through its canonical desktop entry. `dome-link --app-id org.domed.Torrent` presents a reused window in the requested realm before GApplication forwards the input. Separate torrent-file and magnet defaults preserve explicit choices. |
| Penguin Mail | D-Bus calendar/compose activation can reuse a window in the other realm. Desktop entry actions and DomeD's clock launch use the mode-aware helper before reuse. |
| fsvr | Native FLTK inherits its Wayland connection. Normal file handlers now select the current socket. Its explicit desktop action selects the desktop socket even when the working realm is XR. |
| Gmsh | Normal file opening inherits the selected socket. The separate `gmsh-dome-xr` action deliberately opens its stereo mesh view and is retained. |
| Miro, XNEdit-XR, FLTube, cmuxr, Open Movie Editor, htop GUI | No implicit headset-based Wayland redirection found in their ordinary FLTK frontend launch paths. Desktop entries now select the active socket explicitly; executable/media arguments and field codes are retained. FLTube's external mpvr handoff also benefits from mpvr's corrected automatic mode. |
| Arc SBS GUI, default FLTerm Wayland entry, Connect/Input Methods/Wallets frontends | Normal graphical launchers receive the mode-aware socket selection. Background/session services are not converted into per-activation applications. |
| VRCXR and VRChat Launcher | User-installed desktop entries are covered as user-owned overrides. VRCXR's existing native window is presented in the requested realm before protocol activation. VRChat Launcher's GUI follows the selected socket; intentionally launched XR game processes are not changed. |
| FLTerm XR, cmuxr XR Floor, OpenBrush `--xr` | These are explicit XR applications, not automatic link handlers. Their XR placement remains intentional. |
| LightXR, DomeD shell/tray, credential services | Session infrastructure and compositor-owned UI retain their session/service lifecycle. No background daemon or login session is restarted by the update. |
| Removed applications and development-only browser forks | No removed frontend is reinstalled, and experimental/Dillo builds are not replaced. |

Desktop-origin windows retain DomeD's normal shared-projection policy. This change
prevents automatic XR-only launches and makes explicitly activated existing
handler windows visible in the requested realm; it does not change the global
policy for ordinary new desktop windows.

## Packaging

`packaging/link-handlers.json` is the audited launcher registry. The regular-user
staging script patches installed desktop entries without changing MIME
associations, preserves explicit XR actions, and keeps the original application
command and URI field codes. It also stages routing in the existing shell
wrappers for browser, media and audio tools; native application binaries are not
renamed or replaced by wrapper scripts.

```
python3 scripts/stage-link-handlers.py build/link-stage
# Install reviewed system-stage files using the normal system installer.
# Apply prepared user-owned overrides as the regular user, after the helper exists:
python3 scripts/stage-link-handlers.py build/link-stage --apply-user
```

The system staging and installation scripts include the routing helper and
managed system launchers. User overrides are staged separately, checked against
their original hashes and backed up under `$XDG_STATE_HOME/dome/backups` before
being replaced by the regular user.

## Verification

- Private nested compositor with two real native FLTK windows exercises browser
  and Penguin Mail identities, repeated desktop/XR mode changes, stale source
  sockets, existing-window presentation, exact URI argument preservation and an
  unrelated compositor fallback. These are controlled activation fixtures, not
  authenticated mail or VRCXR account sessions.
- Real native mpvr playback is launched in both realms while the simulated XR
  renderer heartbeat remains active. Desktop playback stays desktop-visible;
  XR playback retains the SBS surface identity and transport.
- mpvr routing tests retain explicit XR behavior and cover desktop working mode
  with a connected headset. Arc SBS tests cover automatic image presentation in
  both modes and fail back to local output if mode discovery fails.
- Packaging tests cover URI field codes, idempotence, desktop actions, explicit
  presentation choices, D-Bus executable routing and presenter exemptions.
- All 26 staged installed system/user desktop entries pass desktop-file-validate.
