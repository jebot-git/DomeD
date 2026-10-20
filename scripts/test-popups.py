#!/usr/bin/env python3
"""Spatial popups through real Qt Wayland surfaces on an isolated desktop.
Requires PySide6, python-xlib, Pillow, Xvfb. Does not use the host desktop or headset.
"""
import json
import os
from pathlib import Path
import re
import signal
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from PIL import Image
from Xlib import X, XK, display
from Xlib.ext import xtest

signal.pthread_sigmask(signal.SIG_SETMASK, [])

if os.environ.get("DOME_POPUP_TEST_BUS") != "1":
    os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, *sys.argv],
              {**os.environ, "DOME_POPUP_TEST_BUS": "1"})
root = Path(__file__).resolve().parents[1]
out = root / "artifacts/popups"
out.mkdir(parents=True, exist_ok=True)
xvfb = os.environ.get("DOME_XVFB") or shutil.which("Xvfb") or str(root / ".local-deps/root/usr/bin/Xvfb")
processes, logs = [], []
with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
base = f"http://127.0.0.1:{port}"

def start(args, name, env, **kwargs):
    log = (out / f"{name}.log").open("w")
    logs.append(log)
    child = subprocess.Popen(args, env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT, **kwargs)
    processes.append(child)
    return child

def wait(predicate):
    deadline = time.monotonic() + 12
    last = None
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, KeyError) as error:
            last = error
        time.sleep(.05)
    raise AssertionError("Timed out: " + str(last))

def state():
    with urllib.request.urlopen(base + "/api/state", timeout=2) as response:
        return json.load(response)["scene"]

def action(data):
    request = urllib.request.Request(base + "/api/action", data=json.dumps(data).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=2) as response:
        assert response.status == 200
    time.sleep(.2)

def events():
    return [json.loads(line) for line in (out / "events.jsonl").read_text().splitlines()]

def frame(id):
    with urllib.request.urlopen(base + f"/api/xr-frame/{id}", timeout=2) as response:
        data = response.read()
        return Image.frombytes("RGBA", (int(response.headers["X-Dome-Width"]), int(response.headers["X-Dome-Height"])), data)

try:
    with tempfile.TemporaryDirectory(prefix="dome-popups-") as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        apps = runtime / "data/applications"
        apps.mkdir(parents=True)
        actions = [f"test{i}" for i in range(50)]
        entry = "[Desktop Entry]\nType=Application\nName=Popup Fixture\nExec=/usr/bin/true\nTerminal=false\nActions=" + ";".join(actions) + ";\n"
        entry += "".join(f"[Desktop Action {name}]\nName=Action {i:02}\nExec=/usr/bin/true\n" for i, name in enumerate(actions))
        (apps / "dome-popup-fixture.desktop").write_text(entry)
        rd, wr = os.pipe()
        start([xvfb, "-displayfd", str(wr), "-screen", "0", "1600x1000x24", "-nolisten", "tcp"], "xvfb", os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            display_name = ":" + pipe.readline().strip()
        env = {**os.environ, "DISPLAY": display_name, "XDG_RUNTIME_DIR": directory,
               "XDG_CONFIG_HOME": str(runtime / "config"), "XDG_DATA_HOME": str(runtime / "data"), "DOME_PREVIEW_PORT": str(port),
               "LIBGL_ALWAYS_SOFTWARE": "1", "RUST_LOG": "warn", "DOME_LOGIN_SESSION": "0",
               "DOME_START_XR": "0", "DOME_RENDERER": "gles"}
        for name in ("WAYLAND_DISPLAY", "WAYLAND_SOCKET", "QT_QPA_PLATFORM"):
            env.pop(name, None)
        start([os.environ.get("DOME_COMPOSITOR",str(root / "target/debug/dome-compositor")), "--backend", "nested"], "compositor", env)
        wait(lambda: state()["source"] == "wayland")
        socket_name = re.search(r"Dome Wayland socket: (\S+)", (out / "compositor.log").read_text())[1]
        start([sys.executable, str(root / "scripts/popup-client.py"), str(out / "events.jsonl")], "client",
              {**env, "WAYLAND_DISPLAY": socket_name, "QT_QPA_PLATFORM": "wayland",
               "QT_WAYLAND_CLIENT_BUFFER_INTEGRATION": "shm", "WAYLAND_DEBUG": "1"})
        wait(lambda: len(state()["windows"]) == 1)
        parent = state()["windows"][0]["id"]
        xd = display.Display(display_name)
        def host_window():
            return next((w for w in xd.screen().root.query_tree().children if str(w.get_wm_name() or "").startswith("Dome")), None)
        host = wait(host_window)
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        def key(name):
            code = xd.keysym_to_keycode(ord(name) if len(name) == 1 else XK.string_to_keysym(name))
            for kind in (X.KeyPress, X.KeyRelease):
                xtest.fake_input(xd, kind, code)
                xd.sync()
                time.sleep(.03)
        def click(x, y, button=1):
            geo = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=int(x + geo.x), y=int(y + geo.y))
            xd.sync()
            time.sleep(.1)
            for kind in (X.ButtonPress, X.ButtonRelease):
                xtest.fake_input(xd, kind, button)
                xd.sync()
                time.sleep(.05)
        def screenshot(name):
            geo = host.get_geometry()
            raw = host.get_image(0, 0, geo.width, geo.height, X.ZPixmap, 0xffffffff)
            image = Image.frombytes("RGB", (geo.width, geo.height), raw.data, "raw", "BGRX")
            image.save(out / f"{name}.png")
            return image
        def expect_pixel(image, x, y, color):
            assert image.getpixel((int(x), int(y))) == color, (x, y, image.getpixel((int(x), int(y))), color)

        time.sleep(.5)
        # Compositor-owned menus share the bounded viewport and own keyboard focus.
        click(160, 770, 3)
        time.sleep(.2)
        task_top = screenshot("task-context-top")
        key("End")
        time.sleep(.2)
        task_bottom = screenshot("task-context-bottom")
        assert task_top.crop((4, 36, 300, 740)).tobytes() != task_bottom.crop((4, 36, 300, 740)).tobytes()
        before = len(events())
        key("a")
        time.sleep(.1)
        assert not any(e["kind"] == "key" and e["panel"] == "parent" for e in events()[before:])
        click(160, 48)
        key("Home")
        key("Escape")
        # Context menus use the actual RMB origin, regardless of client positioner hints.
        rect = state()["windows"][0]["desktop"]
        click(rect["x"] + 500, rect["y"] + 300, 3)
        wait(lambda: len(state()["popups"]) == 1)
        context = state()["popups"][0]
        assert context["context_at"] == [500, 300], context
        time.sleep(.15)
        image = screenshot("context-above-click")
        expect_pixel(image, rect["x"] + 500, rect["y"] + 300 - 30, (144, 80, 176))
        before = len(events())
        key("a")
        wait(lambda: any(e["kind"] == "key" and e["panel"] == "context" for e in events()[before:]))
        key("Escape")
        wait(lambda: not state()["popups"])
        key("l")
        click(rect["x"] + 500, rect["y"] + 300, 3)
        wait(lambda: len(state()["popups"]) == 1)
        context = state()["popups"][0]
        assert context["viewport"]["content_height"] > context["viewport"]["height"], context
        assert context["viewport"]["height"] == 704, context
        menu_x, menu_y = rect["x"] + 500 - 120, 36
        time.sleep(.2)
        before_image = screenshot("context-scroll-top")
        click(menu_x + 120, menu_y + 704 - 12)
        wait(lambda: state()["popups"][0]["viewport"]["offset"] == 96)
        time.sleep(.2)
        image = screenshot("context-scroll-down")
        assert image.getpixel((int(menu_x + 20), menu_y + 100)) != before_image.getpixel((int(menu_x + 20), menu_y + 100))
        before = len(events())
        click(menu_x + 100, menu_y + 24 + 20)
        wait(lambda: any(e["kind"] == "press" and e["panel"] == "context" for e in events()[before:]))
        press = next(e for e in events()[before:] if e["kind"] == "press" and e["panel"] == "context")
        assert abs(press["y"] - 116) <= 1, press
        click(menu_x + 120, menu_y + 12)
        wait(lambda: state()["popups"][0]["viewport"]["offset"] == 0)
        key("Escape")
        wait(lambda: not state()["popups"])
        key("l")
        # XR uses the same context trigger, scroll bands and logical content coordinates.
        key("l")
        request = urllib.request.Request(base + "/api/xr-input", data=json.dumps({
            "events": [{"kind":"motion","id":parent,"uv":[.25,.6]},
                       {"kind":"button","button":273,"pressed":True},
                       {"kind":"button","button":273,"pressed":False}],
            "actions":[{"op":"focus","id":parent}]
        }).encode(), headers={"Content-Type":"application/json"})
        with urllib.request.urlopen(request, timeout=2) as response: assert response.status == 200
        wait(lambda: len(state()["popups"]) == 1)
        point = state()["popups"][0]["context_at"]
        assert abs(point[0] - 180) < .01 and abs(point[1] - 276) < .01, point
        context = state()["popups"][0]
        def xr_click(uv):
            request = urllib.request.Request(base + "/api/xr-input", data=json.dumps({
                "events": [{"kind":"motion","id":context["id"],"uv":uv},
                           {"kind":"button","button":272,"pressed":True},
                           {"kind":"button","button":272,"pressed":False}], "actions":[]
            }).encode(), headers={"Content-Type":"application/json"})
            with urllib.request.urlopen(request, timeout=2) as response: assert response.status == 200
        xr_click([.5, 692/704])
        wait(lambda: state()["popups"][0]["viewport"]["offset"] == 96)
        before = len(events())
        xr_click([.5, .5])
        wait(lambda: any(e["kind"] == "press" and e["panel"] == "context" for e in events()[before:]))
        press = next(e for e in events()[before:] if e["kind"] == "press" and e["panel"] == "context")
        assert abs(press["y"] - 424) <= 1, press
        key("Escape")
        wait(lambda: not state()["popups"])
        key("l")
        key("p")
        wait(lambda: len(state()["popups"]) == 1)
        popup = state()["popups"][0]
        assert popup["root"] == parent and popup["parent"] == parent
        assert len(state()["windows"]) == 1
        rect = state()["windows"][0]["desktop"]
        x, y = rect["x"] + popup["anchor"]["x"], rect["y"] + popup["anchor"]["y"]
        time.sleep(.2)
        image = screenshot("desktop-anchored")
        expect_pixel(image, x + 100, y + 100, (54, 163, 129))
        expect_pixel(image, rect["x"] + 100, rect["y"] + 50, (189, 62, 78))
        wait(lambda: frame(popup["id"])).save(out / "popup-texture.png")
        parent_texture = wait(lambda: frame(parent))
        expect_pixel(parent_texture.convert("RGB"), 250, 150, (189, 62, 78))
        click(x + 100, y + 100)
        wait(lambda: any(e["kind"] == "press" and e["panel"] == "popup" for e in events()))
        before = len(events())
        request = urllib.request.Request(base + "/api/xr-input", data=json.dumps({
            "events": [{"kind": "motion", "id": popup["id"], "uv": [.5, .5]},
                       {"kind": "button", "button": 272, "pressed": True},
                       {"kind": "button", "button": 272, "pressed": False}], "actions": []
        }).encode(), headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=2) as response:
            assert response.status == 200
        wait(lambda: any(e["kind"] == "press" and e["panel"] == "popup" for e in events()[before:]))
        press = next(e for e in events()[before:] if e["kind"] == "press")
        assert abs(press["x"] - 120) <= 1 and abs(press["y"] - 90) <= 1, press
        key("n")
        wait(lambda: len(state()["popups"]) == 2)
        child = next(p for p in state()["popups"] if p["id"] != popup["id"])
        assert child["parent"] == popup["id"]
        click(x + child["anchor"]["x"] + 100, y + child["anchor"]["y"] + 50)
        wait(lambda: any(e["kind"] == "press" and e["panel"] == "nested" for e in events()))
        key("Escape")
        wait(lambda: len(state()["popups"]) == 1)
        key("Escape")
        wait(lambda: not state()["popups"])
        key("p")
        wait(lambda: len(state()["popups"]) == 1)
        request = urllib.request.Request(base + "/api/xr-input", data=json.dumps({
            "events": [{"kind": "leave"}, {"kind": "button", "button": 272, "pressed": True},
                       {"kind": "button", "button": 272, "pressed": False}], "actions": []
        }).encode(), headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=2) as response:
            assert response.status == 200
        wait(lambda: not state()["popups"])
        # A popup near the right edge keeps its constrained client anchor.
        action({"op": "desktop_move", "id": parent, "x": 800, "y": 100})
        key("p")
        wait(lambda: len(state()["popups"]) == 1)
        time.sleep(.2)
        image = screenshot("desktop-edge-anchor")
        edge = state()["popups"][0]["anchor"]
        expect_pixel(image, min(800 + edge["x"], 1280 - edge["w"]) + 100,
                     min(100 + edge["y"], 740 - edge["h"]) + 100, (54, 163, 129))
        # An outside click dismisses the protocol popup and returns focus to the owner.
        click(50, 700)
        wait(lambda: not state()["popups"])
        action({"op": "focus", "id": parent})
        key("d")
        wait(lambda: len(state()["windows"]) == 2 and len(state()["popups"]) == 1)
        dialog = state()["popups"][0]
        assert dialog["id"] < (1 << 48) and dialog["root"] == parent
        assert dialog["anchor"]["w"] == 240 and dialog["anchor"]["h"] == 180, dialog
        key("Escape")
        wait(lambda: not state()["popups"])
        key("p")
        wait(lambda: len(state()["popups"]) == 1)
        action({"op": "minimize", "id": parent})
        wait(lambda: not state()["popups"])
        action({"op": "focus", "id": parent})
        key("d")
        wait(lambda: len(state()["windows"]) == 2)
        action({"op": "close", "id": parent})
        wait(lambda: not state()["windows"])
        xd.close()
        print("PASS: RMB context placement and focus; desktop/XR scroll controls and content coordinates; long task menu navigation; nested popups, textures, dismissal, dialogs and cleanup")
finally:
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
