# FLTK 1.4.5 experiment

GPU compositor and DMA-BUF OpenXR transport with damage-aware texture reuse for native windows and shell layers. The headless shell caches Cairo/FreeType rasterization; the sharing picker uses OpenGL FLTK widgets.

This experimental default is preserved in the private `fltk-vr` source
collection. The collection provides a staged local installer with backups;
running desktop and display-manager sessions are never restarted by installation.

```sh
DOME_FLTK_PREFIX=<FLTK prefix> cargo build --features nested,xr --bin dome-compositor --bin dome-xr --bin dome-portal-picker
```

The Qt frontend remains selectable with Cargo `--no-default-features --features nested,qt6`.
Use the isolated FLTK **1.4.5** runtime in the parent `_toolkit`; prepend its `lib`
to `LD_LIBRARY_PATH` so the login session's older runtime cannot override it.
The parent `run` launcher handles this. Local bitmap family/style/strike, popup
dismissal, deferred window close, EGL depth and borderless app-ID patches are required.

Real global menus, native popup policy, coherent vector icons, grouped task state/thumbnail controls, tray attention/pixmaps and notification action geometry are retained. Changed shell contents still rasterize on the CPU before GPU composition. Full theme-icon lookup/text wrapping and real portal consent/stream lifecycle remain incomplete.

Application menu commands are exported over DBusMenu/KDE AppMenu where applicable.
DomeD appearance updates apply live, including native FreeType bitmap strikes.
OpenGL widgets cache glyph textures after rasterization. Bitmap shaping/bidi and
per-glyph fallback, accessibility/IME and broad fractional-scale coverage remain
incomplete. Tests cover selected desktop and DomeD XR input paths, not every
interaction. The local WiVRn server has no connected client: physical Intel GPU
OpenXR validation uses an isolated Monado simulated HMD and does not validate
WiVRn network streaming.

The parent `README.md` contains the suite report, validation commands and remaining
limitations; evidence is in the parent `artifacts/` directory.

Native FLTK file choosers are used without GTK, Zenity or KDialog. Standard
FLTK dialogs inherit the DomeD bitmap font and live palette. The isolated
`first-configure` toolkit patch prevents transient dialogs from freezing while
waiting for a redundant second Wayland configure. Ordinary third-party FLTK
applications need the same appearance hook to follow DomeD automatically.
