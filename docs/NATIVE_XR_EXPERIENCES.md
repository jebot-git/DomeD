# Native XR experiences

Tempest XR is a bundled de-xr experience. Its launcher and icon are staged by
default alongside DomeD, using the revision in `packaging/experiences.json`.
`scripts/stage-experiences.py STAGE` builds as the regular user with Release
optimization and native OpenXR enabled. A matching clean sibling checkout is
reused; otherwise a separate pinned cache is cloned. Local commits and modified
checkouts are never reset. `DOME_BUNDLED_EXPERIENCES=0` selects a minimal
compositor-only distribution explicitly.

The game selects desktop/native XR from the authenticated Wayland socket's
`.dome-session.json` launch record. Explicit desktop and XR launcher actions
select the corresponding socket before SDL connects. Native primary OpenXR
provides the game image; it is not an in-window SBS app or a DomeD overlay.

## Reversible handoff

`GET /api/native-xr` advertises version 1 and the compositor PID. A client posts
`begin`, `heartbeat`, `suspend` or `end`, with its Wayland window ID, peer PID and
a per-process random hexadecimal token (at least 24 characters). Window ownership
must match the compositor's Wayland peer credentials. An active experience cannot
be replaced by another client's token. A paused experience can be replaced.

The compositor saves presentation, workspace, previous focus and companion
visibility. The game companion receives direct seat input while XR desktop and
bars are hidden. Alt+Tab is intercepted before application forwarding, suspends
the lease, restores the desktop snapshot and releases pointer grabs. The game
pauses and destroys its primary OpenXR session so the desktop can regain runtime
focus. Selecting the game reopens native XR; play remains paused until click or
Enter. V explicitly ends the handoff in desktop mode.

Heartbeat expiry (two seconds), window destruction, client death and locking
restore the snapshot. Background window mapping cannot steal capture. The
suspended game is minimized to preserve the previous XR packing; other window
geometry is not rewritten. Session persistence records the saved presentation
and original companion visibility, never the temporary hidden desktop or lease
token. Intentional native takeover does not trigger XR-contingency promotion.

The API is session-local. No display-manager/compositor restart occurs during
installation. A newly installed game stays in desktop mode with a clear message
when the running compositor predates this API.

## Checks

`cargo test --lib` covers restoration, authentication, expiry, background focus,
canonical session persistence and chaining paused experiences. Tempest XR's
`tools/test_domed.py` runs its real SDL/Wayland/OpenXR code under private Xvfb,
D-Bus and DomeD instances with an API-level test runtime. It exercises both
launch realms, wheel input outside the game bounds, Alt+Tab, native re-entry,
V back to desktop, SIGKILL, SIGSTOP expiry and older-compositor rejection.
It never connects to the active headset or changes its runtime. Actual WiVRn
tracking, streaming and headset-focus behavior still need device validation.
