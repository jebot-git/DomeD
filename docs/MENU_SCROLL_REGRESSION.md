# Universal and tray menu scrolling regression

`672f225e98e05e89d40f5a6f641dac7181cd09c1` introduced bounded scroll
viewports for shell/application universal menus on 2026-10-23. Three omissions
left the implementation incomplete:

* StatusNotifier and input-method tray menus still painted/hit-tested every row
  without a viewport. Keyboard navigation changed the selected source row but
  could not reveal it, and XR pullout sizing used the full row count.
* The shell buffer cache tracked menu selection but omitted `menu_offsets`.
  Repeated edge/wheel scrolling with unchanged selection could update input
  coordinates while leaving the old menu pixels visible.
* XR motion cleared edge-hover before the existing stationary-ray guard. The
  next identical sample canceled edge repetition; the guard then returned to
  preserve keyboard selection without restoring the edge state.

Tray menus now use the same bounded Viewport for paint, hover, clicks, wheel,
keyboard reveal and XR sizing. Visual selection/offset changes participate in
the shell cache without invalidating pending D-Bus replies. Both universal
menu offsets and stationary XR edge hover are retained until real movement,
keyboard input, leaving the shell or dismissal changes them. No client gets
keyboard focus when a menu scrolls.

Run `scripts/test-tray-scroll.py` with python-xlib, Pillow and dbus-next. It uses
a private bus, Xvfb, a private nested compositor, an 80-row DBusMenu and an
80-action native FLTK universal menu. It checks End/Home/arrows, actual pointer
hit testing against sparse remote action IDs, repeated identical XR rays,
continuous repainting and wheel scrolling. It does not touch a running session.

Horizontal menu-bar overflow is independent of dropdown scrolling. Exported
headings previously kept their original positions and were simply clipped at
the tray boundary. The application/system buttons now stay fixed while the
headings use a horizontal viewport ending before the visible tray icons. Left
and right controls support clicks and stationary pointer hover; wheel input
also moves the strip. Opening a heading, including Left/Right keyboard menu
navigation, reveals its entire allocated width. Focus changes reset the strip.
Painting, dropdown anchors, hit testing and the shell redraw cache share the
same header offset and viewport, including the 1024-pixel XR shell.

The integration fixture also exports twenty additional menu headings. It
checks horizontal keyboard reveal and action dispatch on desktop and XR, then
holds an XR pointer at the right edge with no dropdown open and clicks an
initially hidden category. `scripts/test-universal-menu-scroll.cpp` checks that
shifted header selection never paints over fixed buttons, controls or the tray.
Set `DOME_MENU_SCROLL_OUTPUT` to retain captures in a separate output directory.
