#!/usr/bin/env python3
"""Exercise the shared file chooser on a private native Wayland display."""

import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get("DOME_PICKER_TEST_BUS") != "1":
    os.execvpe(
        "dbus-run-session",
        ["dbus-run-session", "--", sys.executable, *sys.argv],
        {**os.environ, "DOME_PICKER_TEST_BUS": "1"},
    )
if len(sys.argv) != 3:
    raise SystemExit("Usage: test-wayland.py FILE-PICKER-TEST OUTPUT-DIRECTORY")
binary = Path(sys.argv[1]).resolve()
out = Path(sys.argv[2]).resolve()
out.mkdir(parents=True, exist_ok=True)
children = []
try:
    with tempfile.TemporaryDirectory(prefix="dome-picker-wayland-") as tmp:
        os.chmod(tmp, 0o700)
        rd, wr = os.pipe()
        xvfb = subprocess.Popen(
            [
                "Xvfb",
                "-displayfd",
                str(wr),
                "-screen",
                "0",
                os.environ.get("DOME_PICKER_VIEWPORT", "1600x1000") + "x24",
                "-nolisten",
                "tcp",
            ],
            pass_fds=(wr,),
            stderr=subprocess.DEVNULL,
        )
        children.append(xvfb)
        os.close(wr)
        with os.fdopen(rd) as stream:
            display = ":" + stream.readline().strip()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        env = {
            **os.environ,
            "DISPLAY": display,
            "HOME": tmp,
            "XDG_RUNTIME_DIR": tmp,
            "XDG_CONFIG_HOME": tmp + "/config",
            "XDG_DATA_HOME": tmp + "/data",
            "XDG_STATE_HOME": tmp + "/state",
            "XDG_CURRENT_DESKTOP": "Dome",
            "DOME_START_XR": "0",
            "DOME_START_TRAY": "0",
            "DOME_START_IME": "0",
            "DOME_LOGIN_SESSION": "0",
            "DOME_SESSION_RESTORE": "0",
            "DOME_RENDERER": "gles",
            "DOME_PREVIEW_PORT": str(port),
            "LIBGL_ALWAYS_SOFTWARE": "1",
            "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=/nonexistent/dome-picker-test-bus",
            "XR_RUNTIME_JSON": "/nonexistent/dome-picker-test-runtime.json",
            "GIO_USE_VFS": "local",
            "DOME_PICKER_PRIVATE_RUNTIME": "1",
        }
        for key in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "FLTK_BACKEND"):
            env.pop(key, None)
        subprocess.run(["dbus-update-activation-environment", "HOME", "XDG_RUNTIME_DIR",
                        "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME",
                        "DBUS_SYSTEM_BUS_ADDRESS", "GIO_USE_VFS"], env=env, check=True)
        with (out / "compositor.log").open("w") as log:
            compositor = subprocess.Popen(
                [
                    os.environ.get("DOME_COMPOSITOR", "/usr/local/bin/dome-compositor"),
                    "--backend",
                    "nested",
                ],
                env={**env, "LD_LIBRARY_PATH": os.environ.get("DOME_PICKER_COMPOSITOR_LIBRARY_PATH", env.get("LD_LIBRARY_PATH", ""))},
                stdout=log,
                stderr=log,
            )
            children.append(compositor)
            deadline = time.monotonic() + 20
            while time.monotonic() < deadline:
                match = re.search(
                    r"Dome Wayland socket: (\S+)", (out / "compositor.log").read_text()
                )
                if match:
                    break
                assert compositor.poll() is None, "Private compositor exited"
                time.sleep(0.1)
            else:
                raise AssertionError("Private compositor did not start")
            if "DOME_PICKER_VIEWPORT" in os.environ:
                # Resize only the compositor window on this fixture's private X
                # display. LOGIN_SESSION stays off; no session services run.
                from Xlib import display as xdisplay
                xd = xdisplay.Display(display)
                def locate(parent):
                    for win in parent.query_tree().children:
                        title = win.get_wm_name() or ""
                        if "experimental Wayland compositor" in title:
                            return win
                        found = locate(win)
                        if found is not None:
                            return found
                    return None
                native = locate(xd.screen().root)
                assert native is not None, "Private nested compositor window not found"
                width, height = map(int, os.environ["DOME_PICKER_VIEWPORT"].split("x"))
                native.configure(x=0,y=0,width=width,height=height)
                xd.sync(); time.sleep(.5)
                geometry = native.get_geometry()
                assert (geometry.width,geometry.height)==(width,height), "Nested window did not resize"
                xd.close()
                env["DOME_PICKER_MAX_WORK_AREA"] = os.environ["DOME_PICKER_VIEWPORT"]
            env.update(
                WAYLAND_DISPLAY=match[1], FLTK_BACKEND="wayland", WAYLAND_DEBUG="client"
            )
            with (out / "picker.log").open("w") as capture:
                app = subprocess.Popen(
                    [str(binary), str(out)], env=env, stdout=capture, stderr=capture
                )
                children.append(app)
                app.wait(timeout=180)
            trace = (out / "picker.log").read_text()
            assert app.returncode == 0, trace[-5000:]
            assert "xdg_surface" in trace, "No native Wayland chooser surface"
            assert "PASS shared chooser" in trace, "Chooser checks did not complete"
            print(
                "PASS native Wayland shared picker sorting, file results, resizing and previews"
            )
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=4)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
