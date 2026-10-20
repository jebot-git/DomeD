#!/usr/bin/env python3
"""Ghostty XR integration on a private Xvfb display, runtime directory and D-Bus.
Requires python-xlib, Pillow and Xvfb; build both projects first.
"""
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import threading
import urllib.request

from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image

if os.environ.get("DOME_TERMINAL_TEST_BUS") != "1":
    os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, *sys.argv],
              {**os.environ, "DOME_TERMINAL_TEST_BUS": "1"})

root = Path(__file__).resolve().parents[1]
out = root / "artifacts/terminal"
out.mkdir(parents=True, exist_ok=True)
xvfb = os.environ.get("DOME_XVFB") or shutil.which("Xvfb")
if not xvfb:
    local = root / ".local-deps/root/usr/bin/Xvfb"
    if local.exists():
        xvfb = str(local)
if not xvfb:
    raise SystemExit("Install Xvfb or set DOME_XVFB")
processes, logs = [], []
overlay = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
base = f"http://127.0.0.1:{port}"


def start(args, name, env, **kwargs):
    log = (out / f"{name}.log").open("w")
    logs.append(log)
    child = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child


def wait(predicate):
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(.05)
    raise AssertionError("Timed out waiting for integration state; see artifacts/terminal logs")


def state():
    with urllib.request.urlopen(base + "/api/state", timeout=2) as response:
        return json.load(response)["scene"]


def action(data):
    request = urllib.request.Request(base + "/api/action", data=json.dumps(data).encode(),
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=2) as response:
        assert response.status == 200


try:
    with tempfile.TemporaryDirectory(prefix="dome-terminal-") as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        # Model an existing suspended XR terminal. Desktop launches must leave
        # this socket untouched even with the normal overlay configuration.
        (runtime / "ghostty-xr").mkdir(mode=0o700)
        overlay.bind(str(runtime / "ghostty-xr/control.sock"))
        overlay.settimeout(.2)
        rd, wr = os.pipe()
        start([xvfb, "-displayfd", str(wr), "-screen", "0", "1600x1000x24", "-nolisten", "tcp"],
              "xvfb", os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display_name = ":" + pipe.readline().strip()
        assert display_name != ":"
        env = {**os.environ, "DISPLAY": display_name, "XDG_RUNTIME_DIR": directory,
               "DOME_LOGIN_SESSION": "0", "DOME_BACKEND": "nested", "DOME_START_XR": "0", "DOME_RENDERER": "gles",
               "SDL_VIDEO_WAYLAND_ALLOW_LIBDECOR": "0",
               "XDG_CONFIG_HOME": str(runtime / "config"), "SHELL": "/bin/sh",
               "DOME_PREVIEW_PORT": str(port), "DOME_TERMINAL_MODE": "overlay",
               "DOME_TERMINAL": str(root / "../ghostty-xr/build/ghostty-xr"), "LIBGL_ALWAYS_SOFTWARE": "1", "RUST_LOG": "warn", "SDL_JOYSTICK_DISABLE_UDEV": "1",
               "SDL_HINT_VIDEO_X11_NET_WM_BYPASS_COMPOSITOR": "0",
               # An invalid runtime proves Wayland mode does not initialize OpenXR.
               "XR_RUNTIME_JSON": str(runtime / "no-openxr.json")}
        for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "QT_QPA_PLATFORM"):
            env.pop(name, None)
        start([str(root / "scripts/run-local.sh")], "compositor", env)
        wait(lambda: len(state()["windows"]) == 1)
        window = state()["windows"][0]
        assert window["app_id"] == "org.ghostty.xr.wayland", window
        first = window["id"]
        xd = display.Display(display_name)
        def host_window():
            return next((w for w in xd.screen().root.query_tree().children
                         if str(w.get_wm_name() or "").startswith("Dome")), None)
        host = wait(host_window)
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        time.sleep(.5)

        def key(name, down=None):
            symbol = ord(name) if len(name) == 1 else XK.string_to_keysym(name)
            code = xd.keysym_to_keycode(symbol)
            assert code, name
            for pressed in ([True, False] if down is None else [down]):
                xtest.fake_input(xd, X.KeyPress if pressed else X.KeyRelease, code)
                xd.sync()
                time.sleep(.02)

        # Real host input -> Dome's Wayland seat -> SDL text -> PTY shell.
        for char in "echo domed > " + directory + "/typed":
            code, level = next(iter(xd.keysym_to_keycodes(ord(char))))
            if level & 1:
                key("Shift_L", True)
            xtest.fake_input(xd, X.KeyPress, code)
            xtest.fake_input(xd, X.KeyRelease, code)
            xd.sync()
            if level & 1:
                key("Shift_L", False)
            time.sleep(.02)
        key("Return")
        wait(lambda: (runtime / "typed").exists())
        assert (runtime / "typed").read_text().strip() == "domed"
        # Clipboard shortcuts must use the seat's selection, including UTF-8 paste.
        wayland = re.search(r"Dome Wayland socket: (\S+)", (out / "compositor.log").read_text())[1]
        clipboard_env = {**env, "WAYLAND_DISPLAY": wayland}
        pasted = runtime / "pasted"
        command = "printf '%s' 'Žluťoučký 日本語' > " + str(pasted)
        subprocess.run(["wl-copy", "--type", "text/plain;charset=utf-8"], input=command.encode(),
                       env=clipboard_env, check=True, timeout=3)
        assert subprocess.check_output(["wl-paste", "--no-newline"], env=clipboard_env, timeout=3).decode() == command
        action({"op": "focus", "id": first})
        time.sleep(.3)
        key("Control_L", True); key("Shift_L", True); key("v")
        key("Shift_L", False); key("Control_L", False)
        time.sleep(.2); key("Return")
        wait(lambda: pasted.exists())
        assert pasted.read_text() == "Žluťoučký 日本語"
        key("Control_L", True); key("Shift_L", True); key("a"); key("c")
        key("Shift_L", False); key("Control_L", False)
        copied = subprocess.check_output(["wl-paste", "--no-newline"], env=clipboard_env, timeout=3).decode()
        assert "domed" in copied, copied
        # Exercise the windowless overlay backend without opening a headset or input device.
        helper = root / "../ghostty-xr/build/clipboard-test"
        subprocess.run([str(helper)], env={**clipboard_env, "GHOSTTY_CLIPBOARD_INTEGRATION": "1"},
                       check=True, timeout=8)
        key("Super_L", True)
        key("t")
        key("Super_L", False)
        wait(lambda: len(state()["windows"]) == 2)
        assert len(list((runtime / "ghostty-xr").glob("wayland-*.sock"))) == 2
        key("Super_L", True); key("F12"); key("Super_L", False)
        wait(lambda: len(state()["windows"]) == 3)
        assert len(list((runtime / "ghostty-xr").glob("wayland-*.sock"))) == 3

        def frame():
            with urllib.request.urlopen(base + f"/api/xr-frame/{first}", timeout=2) as response:
                data = response.read()
                width = int(response.headers["X-Dome-Width"])
                height = int(response.headers["X-Dome-Height"])
                assert len(data) == width * height * 4
                image = Image.frombytes("RGBA", (width, height), data)
                assert len(image.getcolors(width * height) or []) > 10
                image.save(out / "terminal.png")
                return True

        wait(frame)
        action({"op": "maximize", "id": first})
        wait(lambda: next(w for w in state()["windows"] if w["id"] == first)["maximized"])
        action({"op": "minimize", "id": first})
        wait(lambda: next(w for w in state()["windows"] if w["id"] == first)["minimized"])
        action({"op": "focus", "id": first})
        wait(lambda: state()["focused"] == first)
        for window in state()["windows"]:
            action({"op": "close", "id": window["id"]})
        wait(lambda: not state()["windows"])
        wait(lambda: not list((runtime / "ghostty-xr").glob("wayland-*.sock")))
        try:
            overlay.recv(1024)
            raise AssertionError("Desktop terminal launch sent a command to the suspended XR terminal")
        except socket.timeout:
            pass
        # A private XR heartbeat gives the nested compositor XR shortcut ownership.
        stopped = threading.Event()
        def heartbeat():
            while not stopped.is_set():
                request = urllib.request.Request(base + "/api/xr-session", data=json.dumps({
                    "id": 987, "visible": True, "focused": True,
                    "head": {"position_m": [0, 1.4, 0], "orientation_xyzw": [0, 0, 0, 1]},
                    "horizontal_fov_deg": 100}).encode(), headers={"Content-Type": "application/json"})
                with urllib.request.urlopen(request, timeout=2):
                    pass
                stopped.wait(.1)
        pulse = threading.Thread(target=heartbeat, daemon=True)
        pulse.start()
        try:
            def xr_owned():
                with urllib.request.urlopen(base + "/api/xr-control", timeout=2) as response:
                    return json.load(response)["input_realm"] == "xr"
            wait(xr_owned)
            for logo in (False, True):
                if logo: key("Super_L", True)
                key("F12")
                if logo: key("Super_L", False)
                assert overlay.recv(1024) == b"show"
                try:
                    overlay.recv(1024)
                    raise AssertionError("F12 reopening created a tab or repeated the request")
                except socket.timeout:
                    pass
            assert not state()["windows"]
        finally:
            stopped.set()
            pulse.join(timeout=3)
        xd.close()
        print("PASS: independent desktop terminal, clipboard copy/UTF-8 paste, windowless clipboard roundtrip, Wayland seat typing, Super+T, F12/Super+F12 reopen without new tabs, XR texture bridge, window lifecycle and socket cleanup")
finally:
    overlay.close()
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
    for child in reversed(processes):
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    for log in logs:
        log.close()
