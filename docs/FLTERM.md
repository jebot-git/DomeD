# FLTerm terminal backend

The [FLTerm XR fork](https://github.com/jebot-git/flterm-xr) uses FLTerm's own
FLTK/VT100 core for independent desktop Wayland terminals and a persistent native
OpenXR overlay. It shares the established DomeD terminal command and realm policy.

With FLTerm installed on `PATH`, DomeD selects it unless an explicit terminal or
backend override is configured. `DOME_TERMINAL_BACKEND=flterm|ghostty` chooses the
protocol/control namespace; `DOME_TERMINAL=/absolute/executable` chooses a custom
binary. A custom executable named `flterm-xr*` selects the FLTerm protocol
automatically. Existing explicit Ghostty/custom-terminal paths retain their
previous behavior. The session launcher delegates selection to this policy.

- Desktop **Super+T** / **Open Terminal**: another independent FLTK Wayland window.
- XR **Super+T** / **Open Terminal**: create/select a tab in the current overlay.
- **Super+F12**: show the selected overlay without creating a tab in either realm.
- Bare **F12** belongs to the overlay only in the XR realm; desktop clients retain it.
- `DOME_TERMINAL_MODE=wayland` creates compositor windows in both realms; the
  explicit overlay show shortcut remains available.

FLTerm's overlay uses `$XDG_RUNTIME_DIR/flterm-xr/control.sock`; Ghostty's uses
`ghostty-xr/control.sock`. Backend selection applies to reuse, show, cold start and
stale endpoint recovery and seat-deactivation hide, preventing one terminal from controlling the other's
shells. The FLTK desktop window exports its real commands into the universal menu
and follows DomeD Appearance, BDF face/strike, integer scaling and clipboard.

`cargo test --lib terminal::tests`, `cargo test --test terminal_overlay` and
`cargo test --test terminal_flterm` cover backend selection, preserved overrides,
realm/F12 behavior, and separate live FLTerm/Ghostty datagram endpoints.
The FLTerm repository supplies real PTY, Wayland/clipboard and simulated native
OpenXR tests. Its README records remaining VT100, advanced IME and physical
headset validation limits; this backend does not use the libghostty VT engine.
