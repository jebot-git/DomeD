#!/usr/bin/env python3
"""Real desktop-switcher clicks/keys and XR pointer input on a private compositor.

Run with: uv run --no-project --with python-xlib --with pillow python scripts/test-workspaces.py
"""

import json
import os
from pathlib import Path
import re
import shlex
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from PIL import Image, ImageChops
from Xlib import X, XK, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get("DOME_WORKSPACE_TEST_BUS") != "1":
    os.execvpe(
        "dbus-run-session",
        ["dbus-run-session", "--", sys.executable, *sys.argv],
        {**os.environ, "DOME_WORKSPACE_TEST_BUS": "1"},
    )
root = Path(__file__).resolve().parents[1]
out = root / "artifacts/workspaces"
out.mkdir(parents=True, exist_ok=True)
processes = []
checks = []
xd = None


def check(okay, message):
    assert okay, message
    checks.append(message)
    print("PASS " + message, flush=True)


def wait(fn, seconds=15):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(0.05)
    raise AssertionError("Timed out waiting for desktop-switcher state")


def layout(width, height, count):
    rows = count + (count < 8)
    row = max(16, min(36, (height - 166) // rows))
    w = min(560, max(1, width - 20))
    h = rows * row + 60
    return width - w - 10, max(36, height - 70 - h), w, h, row


try:
    cairo_flags = shlex.split(
        subprocess.check_output(["pkg-config", "--cflags", "cairo"], text=True)
    )
    subprocess.run(
        [
            "c++",
            "-std=c++17",
            "-O2",
            str(root / "tests/workspaces/client.cpp"),
            "-I/opt/fltk-vr/toolkit/include",
            "-L/opt/fltk-vr/toolkit/lib",
            "-Wl,-rpath,/opt/fltk-vr/toolkit/lib",
            "-lfltk",
            "-o",
            str(out / "client"),
        ]
        + cairo_flags,
        check=True,
    )
    with tempfile.TemporaryDirectory(prefix="dome-workspaces-") as tmp:
        os.chmod(tmp, 0o700)
        rd, wr = os.pipe()
        xvfb = subprocess.Popen(
            [
                "Xvfb",
                "-displayfd",
                str(wr),
                "-screen",
                "0",
                "1600x1000x24",
                "-nolisten",
                "tcp",
                "-noreset",
            ],
            pass_fds=(wr,),
            stderr=subprocess.DEVNULL,
        )
        processes.append(xvfb)
        os.close(wr)
        with os.fdopen(rd) as stream:
            ds = ":" + stream.readline().strip()
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            port = s.getsockname()[1]
        base = f"http://127.0.0.1:{port}"
        env = {
            **os.environ,
            "DISPLAY": ds,
            "XDG_RUNTIME_DIR": tmp,
            "HOME": tmp,
            "XDG_CONFIG_HOME": tmp + "/config",
            "XDG_DATA_HOME": tmp + "/data",
            "XDG_STATE_HOME": tmp + "/state",
            "XDG_DATA_DIRS": tmp + "/empty",
            "XDG_CURRENT_DESKTOP": "Dome",
            "DOME_START_XR": "0",
            "DOME_START_TRAY": "0",
            "DOME_START_IME": "0",
            "DOME_LOGIN_SESSION": "0",
            "DOME_SESSION_RESTORE": "1",
            "DOME_APPEARANCE_SERVICE": "0",
            "DOME_RENDERER": "gles",
            "LIBGL_ALWAYS_SOFTWARE": "1",
            "DOME_PREVIEW_PORT": str(port),
            "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=/nonexistent/workspace-test-bus",
            "XR_RUNTIME_JSON": "/nonexistent/workspace-test-runtime.json",
        }
        for key in ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "FLTK_BACKEND"]:
            env.pop(key, None)

        def start(args, label, environment=env):
            log = (out / (label + ".log")).open("w")
            p = subprocess.Popen(args, env=environment, stdout=log, stderr=log)
            processes.append(p)
            return p

        compositor = start(
            [
                os.environ.get(
                    "DOME_COMPOSITOR", str(root / "target/release/dome-compositor")
                ),
                "--backend",
                "nested",
            ],
            "compositor",
        )
        match = wait(
            lambda: re.search(
                r"Dome Wayland socket: (\S+)", (out / "compositor.log").read_text()
            )
        )
        client = {**env, "WAYLAND_DISPLAY": match[1], "FLTK_BACKEND": "wayland"}

        def get(path):
            return json.load(urllib.request.urlopen(base + path, timeout=3))

        def state():
            return get("/api/state")["scene"]

        def post(path, value, status=200):
            request = urllib.request.Request(
                base + path,
                data=json.dumps(value).encode(),
                headers={"Content-Type": "application/json"},
            )
            try:
                with urllib.request.urlopen(request, timeout=3) as response:
                    assert response.status == status
            except urllib.error.HTTPError as e:
                assert e.code == status, e.read().decode()

        def action(value, status=200):
            post("/api/action", value, status)
            time.sleep(0.1)

        xd = display.Display(ds)
        host = wait(
            lambda: next(
                (
                    w
                    for w in xd.screen().root.query_tree().children
                    if str(w.get_wm_name() or "").startswith("Dome")
                ),
                None,
            )
        )
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        wait(state)

        def key(name, down=None):
            code = xd.keysym_to_keycode(XK.string_to_keysym(name))
            assert code, name
            for press in [True, False] if down is None else [down]:
                xtest.fake_input(xd, X.KeyPress if press else X.KeyRelease, code)
                xd.sync()
                time.sleep(0.025)

        def text(value):
            for c in value:
                if c.isupper():
                    key("Shift_L", True)
                    key(c.lower())
                    key("Shift_L", False)
                else:
                    key("space" if c == " " else c)

        def select_all():
            key("Control_L", True)
            key("a")
            key("Control_L", False)

        def click(x, y):
            g = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=int(g.x + x), y=int(g.y + y))
            xd.sync()
            time.sleep(0.08)
            for press in [True, False]:
                xtest.fake_input(xd, X.ButtonPress if press else X.ButtonRelease, 1)
                xd.sync()
                time.sleep(0.06)

        def shot(name=None):
            time.sleep(0.18)
            g = host.get_geometry()
            raw = host.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xFFFFFFFF)
            image = Image.frombytes("RGB", (g.width, g.height), raw.data, "raw", "BGRX")
            if name:
                image.save(out / (name + ".png"))
            return image

        def geometry():
            g = host.get_geometry()
            return layout(g.width, g.height, len(state()["workspaces"]))

        def open_switcher():
            key("Escape")
            key("Escape")
            g = host.get_geometry()
            click(g.width - 180, g.height - 30)
            time.sleep(0.15)

        def row_click(index, button="name"):
            x, y, w, h, r = geometry()
            px = {
                "name": x + 50,
                "rename": x + w - 48,
                "save": x + w - 130,
                "cancel": x + w - 48,
            }[button]
            click(px, y + 32 + index * r + r / 2)

        def count_region(image, index):
            x, y, w, h, r = geometry()
            return image.crop(
                (x + w - 190, y + 32 + index * r, x + w - 94, y + 32 + (index + 1) * r)
            )

        def differs(a, b):
            return ImageChops.difference(a, b).getbbox() is not None

        def field_region(image, index):
            x, y, w, h, r = geometry()
            return image.crop(
                (x + 24, y + 32 + index * r, x + w - 180, y + 32 + (index + 1) * r)
            )

        def footer_region(image):
            x, y, w, h, r = geometry()
            return image.crop((x + 12, y + h - 27, x + w - 12, y + h - 3))

        def spawn(title):
            start([str(out / "client"), title], title, client)
            return wait(
                lambda: next(
                    (w["id"] for w in state()["windows"] if w["title"] == title), None
                )
            )

        open_switcher()
        empty = shot("01-empty")
        one = spawn("Workspace A")
        open_switcher()
        single = shot("02-one-window")
        check(
            differs(count_region(empty, 0), count_region(single, 0)),
            "Switcher indicates an open window",
        )
        action({"op": "minimize", "id": one})
        open_switcher()
        minimized = shot()
        check(
            not differs(count_region(single, 0), count_region(minimized, 0)),
            "Minimized windows remain counted as open",
        )
        two = spawn("Workspace B")
        open_switcher()
        multiple = shot("03-two-windows")
        check(
            differs(count_region(single, 0), count_region(multiple, 0)),
            "Window count updates after another window opens",
        )
        action({"op": "move", "id": two, "workspace": 1, "lane": 0})
        open_switcher()
        moved = shot("04-moved-window")
        check(
            not differs(count_region(single, 0), count_region(moved, 0))
            and differs(count_region(empty, 1), count_region(moved, 1)),
            "Moving windows updates both desktop indicators",
        )
        action({"op": "window_visibility", "id": two, "desktop": False, "xr": True})
        open_switcher()
        hidden = shot()
        check(
            not differs(count_region(moved, 1), count_region(hidden, 1)),
            "Switcher counts open windows across desktop and XR presentation",
        )
        action({"op": "close", "id": two})
        wait(lambda: all(w["id"] != two for w in state()["windows"]))
        open_switcher()
        closed = shot("05-closed-window")
        check(
            not differs(count_region(empty, 1), count_region(closed, 1)),
            "Closing the last window restores the empty indicator",
        )
        active = state()["active_workspace"]
        before_edit = shot()
        row_click(1, "rename")
        editor = shot("06-rename-editor")
        check(
            differs(field_region(before_edit, 1), field_region(editor, 1)),
            "Opening rename visibly draws the input field and selection",
        )
        select_all()
        text("Work bench")
        typed = shot()
        check(
            differs(field_region(editor, 1), field_region(typed, 1)),
            "Typed text refreshes the visible editor without a scene change",
        )
        key("Return")
        wait(lambda: state()["workspaces"][1]["name"] == "Work bench")
        check(
            state()["active_workspace"] == active,
            "Rename button and Enter rename another desktop without switching",
        )
        key("F2")
        select_all()
        text("Discarded")
        key("Escape")
        check(
            state()["workspaces"][1]["name"] == "Work bench",
            "F2 opens rename and Escape cancels without mutation",
        )
        key("F2")
        select_all()
        key("BackSpace")
        before_error = shot()
        key("Return")
        invalid = shot("06-invalid-name")
        check(
            state()["workspaces"][1]["name"] == "Work bench"
            and differs(footer_region(before_error), footer_region(invalid)),
            "An empty name is rejected without closing the editor",
        )
        text("Saved by mouse")
        row_click(1, "save")
        wait(lambda: state()["workspaces"][1]["name"] == "Saved by mouse")
        check(True, "Save button commits the corrected name")
        action({"op": "rename_workspace", "workspace": 1, "name": "Práce 日本語"})
        open_switcher()
        row_click(1, "rename")
        shot("07-unicode-editor")
        key("End")
        key("BackSpace")
        key("Return")
        wait(lambda: state()["workspaces"][1]["name"] == "Práce 日本")
        shot("07-unicode-name")
        check(
            True,
            "Unicode rename editing removes a whole character and retains the remainder",
        )
        action({"op": "rename_workspace", "workspace": 1, "name": "日" * 33}, 400)
        action({"op": "rename_workspace", "workspace": 1, "name": "a\nb"}, 400)
        check(
            state()["workspaces"][1]["name"] == "Práce 日本",
            "Invalid remote rename requests leave the desktop unchanged",
        )
        saved = Path(tmp) / "state/dome/session.json"
        wait(
            lambda: (
                saved.exists()
                and json.loads(saved.read_text())["layout"]["workspaces"][1]["name"]
                == "Práce 日本"
            )
        )
        check(True, "Renamed desktop is written to the session layout")
        row_click(1)
        wait(lambda: state()["active_workspace"] == 1)
        check(True, "A normal row click still switches desktops")
        while len(state()["workspaces"]) < 8:
            action(
                {
                    "op": "add_workspace",
                    "name": "Desktop " + str(len(state()["workspaces"]) + 1),
                }
            )
        open_switcher()
        shot("08-eight-desktops")
        key("End")
        key("Return")
        wait(lambda: state()["active_workspace"] == 7)
        check(
            len(state()["workspaces"]) == 8,
            "Eight-desktop switcher selects the last desktop without an extra invalid Add row",
        )

        # The same switcher and rename buttons receive the XR seat's bottom-panel coordinates.
        def xr_click(id, uv):
            post(
                "/api/xr-input",
                {
                    "events": [
                        {"kind": "motion", "id": id, "uv": uv},
                        {"kind": "button", "button": 272, "pressed": True},
                        {"kind": "button", "button": 272, "pressed": False},
                    ],
                    "actions": [],
                },
            )
            time.sleep(0.25)

        shell = get("/api/xr-shell")
        xr_click(2**64 - 3, [(shell["width"] - 180) / shell["width"], 0.5])
        shell = wait(
            lambda: (
                (v if v["bottom"] > 60 else None)
                if (v := get("/api/xr-shell"))
                else None
            )
        )
        x, y, w, h, r = layout(shell["width"], shell["height"], 8)

        def xr_row(button):
            px = x + w - 48 if button in ["rename", "cancel"] else x + w - 130
            py = y + 32 + 7 * r + r / 2
            xr_click(
                2**64 - 7,
                [
                    px / shell["width"],
                    (py - (shell["height"] - shell["bottom"])) / (shell["bottom"] - 60),
                ],
            )

        xr_row("rename")
        select_all()
        text("XR workspace")
        xr_row("save")
        wait(lambda: state()["workspaces"][7]["name"] == "XR workspace")
        check(True, "XR bottom-panel pointer can open and save a desktop rename")
        key("F2")
        select_all()
        text("Stale draft")
        action({"op": "rename_workspace", "workspace": 7, "name": "External change"})
        key("Return")
        check(
            state()["workspaces"][7]["name"] == "External change",
            "An external desktop change is not overwritten by a stale rename draft",
        )
        key("Escape")
        key("Escape")
        xd.close()
        xd = None
        (out / "report.json").write_text(
            json.dumps({"passed": len(checks), "checks": checks}, indent=2) + "\n"
        )
except Exception:
    if xd is not None:
        try:
            shot("failure")
        except Exception:
            pass
    raise
finally:
    if xd is not None:
        xd.close()
    for p in reversed(processes):
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(timeout=4)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait()
