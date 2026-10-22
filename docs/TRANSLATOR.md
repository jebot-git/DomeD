# DomeD Translator

`dome-translator` is a native FLTK 1.4.5 tray application using the installed
system toolkit, DomeD's live palette and the shared BDF font renderer. It has no
Qt frontend and does not bundle a replacement FLTK. Launch it from the application
menu or run `dome-translator`; `--tray` starts hidden, `--clipboard` and `--primary`
open the existing instance and paste the corresponding selection. Closing the
window hides it; **Quit** in its tray menu exits the application. `--selection`
toggles selected-text translation in the existing instance.

The From/To menus follow the current Fcitx5 input group over D-Bus. When Fcitx is
unavailable, the DomeD or standard Fcitx5 profile is used. **Refresh languages**
re-reads the group after changing input methods. English, Japanese, Czech,
Serbian and Korean are discovered from the current system setup. Other keyboard
layouts and input-method `LangCode` metadata are also recognized. DeepL's live
language capabilities determine which configured languages can be used as source
and target; unsupported languages are marked unavailable, without substitution.

Enter text directly, **Paste clipboard** (Ctrl+V), or **Paste primary selection**
(Ctrl+Shift+V or middle-click). On Wayland, `wl-clipboard` provides the explicit
primary-selection transport independently of FLTK's clipboard. Native X11 uses
FLTK's separate buffers. Translation and transcription have separate copy buttons
for the clipboard and primary selection. Pasting does not translate automatically.
Press **Translate** or Ctrl+Enter to send the input to DeepL API Free. Requests run
in separate workers so the editor and tray stay responsive. Input/language changes
invalidate in-flight results, and a billable translation POST is never retried
automatically. There is no history file and no logging of text or credentials.

## Selected-text tooltip

**Translate selected text in popup** is an on/off checkbox in the translator.
The tray menu offers the same checked toggle, and `dome-translator --selection`
toggles it without opening the editor. It starts **off** on each application
launch and is not saved as an automatic opt-in for future sessions.

While on, it monitors the primary selection, waits at least 650 ms for the text
to settle, and sends the selected text to DeepL using the current From/To
languages. A native FLTK tooltip shows the translation and optional reading.
The normal clipboard is kept separate. Unchanged selections are not sent
repeatedly; empty selections, selections over 20,000 UTF-8 bytes and the
translator's own last output are ignored. A failed translation is not retried
automatically. Selecting text while the toggle is off does not start workers or
send text to DeepL.

Turning off immediately stops monitoring and hides the tooltip. Already submitted
HTTP requests may finish, but their results are discarded. Changing the selection
invalidates any older result. **Copy translation** copies the full translated
text to the clipboard without replacing the primary selection. **Open in
translator** transfers the full source, translation and readings to the editor
without sending another request. Long results are bounded in the tooltip; open
them in the editor to read the entire result.

The tooltip uses DomeD's live FLTK font, palette and accent. It expires after
20 seconds away from the pointer, and can be closed directly. DomeD also dismisses
it on Escape or an outside click. It inherits the source window's workspace and
visibility, stays out of taskbar/window cycling, and does not take keyboard focus.
In XR it uses the source panel's popup geometry and projected cursor origin,
retaining the spatial popup beam. Outside DomeD, FLTK supplies the native tooltip
window hint and screen-bound placement.

**Settings** accepts a DeepL API Free key ending in `:fx`. It is stored at
`$XDG_CONFIG_HOME/dome/translator/deepl-key` (default `~/.config/...`), mode 0600,
using atomic replacement. **Remove saved key** deletes it. Alternatively, supply
`DEEPL_API_KEY` in the launch environment; it takes precedence over the saved key.
The key is never placed on a command line, displayed, or returned by the worker.
The endpoint is fixed to `https://api-free.deepl.com`; redirects are rejected.
Authentication, quota, rate-limit, network and unsupported-language errors are
shown in the status line. A key is required for live translation.

The original script is always retained. Japanese additionally gets dictionary-based
Hepburn readings from local pykakasi data; Korean gets ICU's local Hangul-to-Latin
transliteration. Other configured non-Latin/non-Cyrillic scripts use ICU Any-Latin.
Readings are aids, not phonetic guarantees: Japanese names/readings may be ambiguous,
and ICU transliteration does not promise pronunciation for every language.
**English, Czech and Serbian never get transliteration**, including Serbian in
both Cyrillic and Latin script. Other Latin/Cyrillic scripts also leave the reading
blank. A transcription failure preserves the successful translation.

## Build and install

Required system packages: installed patched FLTK 1.4.5 with its CMake config,
C++17, CMake, Ninja, GIO, Fontconfig, zlib, nlohmann-json headers, Python 3 with pip,
ICU runtime libraries, `gdbus`, and `wl-clipboard` for Wayland. The build script
fetches pinned Python reading dependencies into the build directory; it does not
modify system Python. Their licenses are included alongside the installed modules.

```sh
./scripts/build-translator.sh
python3 tests/translator/integration.py
./scripts/stage-translator.sh
python3 scripts/install-translator.py build/translator-system-stage --root build/translator-install-test
SUDO_ASKPASS=/usr/local/bin/dome-askpass sudo -A python3 scripts/install-translator.py build/translator-system-stage
```

The default prefix is `/usr/local`, with the worker and reading dictionaries in
`/usr/local/libexec/dome-translator/`. The stage script records file SHA-256 hashes
and modes; the installer verifies these, installs atomically per file, and retains
rollback copies. `--root DIRECTORY` rehearses the same install without sudo.
For a user-local installation instead, set `DOME_TRANSLATOR_PREFIX="$HOME/.local"`
before building and use `cmake --install build/translator` after inspecting a
DESTDIR stage. Neither installation nor the app restarts DomeD or a display-manager
session. Autostart is optional; launch `dome-translator --tray` from your preferred
session startup configuration.

Core tests mock DeepL and use a local HTTP server to verify protocol/authentication,
redirect refusal, errors, scripts, readings and key storage. Integration tests run
native widgets on private Xvfb and nested DomeD Wayland displays, testing independent
UTF-8 selections, output publication, tray D-Bus interfaces, single instance,
responsiveness and stale-result rejection. They never access the user's clipboard
or DeepL account. `dome-translator --smoke-test FILE.ppm` produces a local preview.

API references: [DeepL text translation](https://developers.deepl.com/api-reference/translate/request-translation),
[DeepL language discovery](https://developers.deepl.com/docs/languages/using-the-languages-api).
