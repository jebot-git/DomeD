# Comic Chat stereo window transport

`org.de_xr.ComicChat.Sbs` opts the main window into squeezed side-by-side XR
composition. The desktop compositor continues to display the normal client
surface. Comic Chat packs two complete eye views only during XR presentation;
its dialogs use the mono `org.de_xr.ComicChat` identity. Both eye copies of the
controls share the original logical input coordinates.

The application window retains ordinary dome curvature, lane spacing and focus
outsets. The integration reports that same cylinder for client-side per-eye
reprojection of the 3D chat viewport. Controls and text preserve normal window
UVs. Pointer hit tests use the unchanged compositor surface geometry.

`GET /api/integrations/comic-chat` advertises version 1, compositor identity,
application identity, current window geometry, head pose and optional eye poses.
The session heartbeat accepts `eyes` only with valid poses and a 35–90 mm
separation. `Control.stereo_views` advertises this heartbeat extension; a new XR
renderer omits it when communicating with an older compositor's strict schema.
Missing or temporarily invalid eye tracking leaves the session usable with the
client's 64 mm fallback baseline. Clients poll outside their UI/render thread;
this transport does not provide predicted-frame late latching.

Validation covers stereo identity and eye texture regions, heartbeat validation
and backward compatibility, pulled-window geometry and pointer hit tests, plus
Comic Chat's real FLTK/OpenGL render on an isolated DomeD. The app test captures
both eye images, checks differing artwork and identical controls, and cycles
back to the desktop view without creating a separate OpenXR presenter.

Build the compositor and XR renderer as an optimized release with the normal
FLTK prefix and `multi-gpu,xr` (or `standalone,xr`) features. Both binaries must
be installed for runtime eye tracking; the updated compositor alone supports
stereo with the fallback baseline. Stage and verify them before system
installation. Leave the active compositor running; use the update on a later
session start.

fsvr uses the same contract at `GET /api/integrations/fsvr`, with the opt-in
identity `org.fsvxr.fsvr.Sbs`. Its FLTK browser packs the full window, renders
filesystem geometry per eye, and reprojects only its 3D viewport onto the
reported cylinder. The saved **View → Stereo 3D** option controls depth; turning
it off keeps identical window images in both eyes. Desktop presentation is mono.
