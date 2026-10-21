#!/usr/bin/env python3
"""Real FLTK/SDL3 Wayland popups on an isolated desktop and simulated XR input.

Build scripts/{fltk,sdl}-popup-client.cpp first. Run with uv --with python-xlib
--with pillow. DOME_TEST_TOOLKIT=fltk|sdl; DOME_TOOLKIT_CLIENT overrides the client.
"""
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get("DOME_NATIVE_POPUP_BUS") != "1":
    os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, *sys.argv],
              {**os.environ, "DOME_NATIVE_POPUP_BUS": "1"})
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image

root = Path(__file__).resolve().parents[1]
toolkit = os.environ.get("DOME_TEST_TOOLKIT", "sdl")
assert toolkit in ("sdl", "fltk")
out = Path(os.environ.get("DOME_TEST_OUTPUT_DIR", str(root / "artifacts" / (toolkit + "-popups"))))
out.mkdir(parents=True, exist_ok=True)
processes, logs, checks = [], [], []
stop = threading.Event()
pulse = None

def start(args, name, env, **kwargs):
    log = (out / (name + ".log")).open("w")
    logs.append(log)
    process = subprocess.Popen(args, env=env, stdout=log, stderr=log,
                               start_new_session=True, **kwargs)
    processes.append(process)
    return process

def wait(predicate, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = predicate()
            if result:
                return result
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(.05)
    raise AssertionError("Timed out; see " + str(out))

def check(ok, label):
    checks.append({"passed": bool(ok), "check": label})
    print(("PASS " if ok else "FAIL ") + label, flush=True)
    assert ok, label

try:
    with tempfile.TemporaryDirectory(prefix="dome-native-popup-") as tmp:
        Path(tmp).chmod(0o700)
        rd, wr = os.pipe()
        start([os.environ.get("DOME_XVFB", "Xvfb"), "-displayfd", str(wr),
               "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-noreset"],
              "xvfb", os.environ.copy(), pass_fds=(wr,))
        os.close(wr)
        with os.fdopen(rd) as pipe:
            ds = ":" + pipe.readline().strip()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        env = {**os.environ, "HOME": tmp, "XDG_RUNTIME_DIR": tmp,
               "XDG_CONFIG_HOME": tmp + "/config", "XDG_DATA_HOME": tmp + "/data",
               "XDG_STATE_HOME": tmp + "/state", "DISPLAY": ds,
               "DOME_LOGIN_SESSION": "0", "DOME_START_XR": "0", "DOME_START_IME": "0",
               "DOME_RENDERER": "gles", "DOME_PREVIEW_PORT": str(port),
               "LIBGL_ALWAYS_SOFTWARE": "1", "DOME_MENU_TRACE": "1"}
        for key in ["WAYLAND_DISPLAY", "WAYLAND_SOCKET", "QT_QPA_PLATFORM", "GTK_MODULES"]:
            env.pop(key, None)
        start([os.environ.get("DOME_COMPOSITOR", str(root / "build/menu-release-candidate/dome-compositor")),
               "--backend", "nested"], "compositor", env)
        base = f"http://127.0.0.1:{port}"
        def get(path):
            return json.load(urllib.request.urlopen(base + path, timeout=2))
        def scene():
            return get("/api/state")["scene"]
        def post(path, value):
            urllib.request.urlopen(urllib.request.Request(base + path,
                data=json.dumps(value).encode(), headers={"Content-Type": "application/json"}), timeout=2).close()
        wait(lambda: scene()["source"] == "wayland")
        socket_name = wait(lambda: re.search(r"Dome Wayland socket: (\S+)",
                             (out / "compositor.log").read_text()))[1]
        client_env = {**env, "WAYLAND_DISPLAY": socket_name, "DISPLAY": "",
                      "SDL_VIDEO_DRIVER": "wayland", "SDL_RENDER_DRIVER": "software", "WAYLAND_DEBUG": "1"}
        if toolkit == "fltk" and os.environ.get("DOME_FLTK_LIBRARY_DIR"):
            client_env["LD_LIBRARY_PATH"] = os.environ["DOME_FLTK_LIBRARY_DIR"] + ":" + env.get("LD_LIBRARY_PATH", "")
        client = start([os.environ.get("DOME_TOOLKIT_CLIENT", str(root / "build/toolkit-popups" / (toolkit + "-popup-client")))],
                       "client", client_env)
        aid = wait(lambda: next((w["id"] for w in scene()["windows"] if w["title"].endswith("fixture")), None))
        # SDL can replace its initial shell surface during libdecor negotiation.
        time.sleep(2)
        aid = wait(lambda: next((w["id"] for w in scene()["windows"] if w["title"].endswith("fixture")), None))
        check(client.poll() is None, toolkit + " maps a native Wayland toplevel with no X11 display")
        xd = display.Display(ds)
        host = wait(lambda: next((w for w in xd.screen().root.query_tree().children
                                  if str(w.get_wm_name() or "").startswith("Dome")), None))
        host.set_input_focus(X.RevertToParent, X.CurrentTime)
        xd.sync()
        def motion(x, y):
            g = host.get_geometry()
            # Crossing a surface yields enter; a subsequent move drives native
            # hover handlers (FLTK does not select menu rows on enter alone).
            xtest.fake_input(xd, X.MotionNotify, x=int(g.x + x - 2), y=int(g.y + y)); xd.sync(); time.sleep(.1)
            xtest.fake_input(xd, X.MotionNotify, x=int(g.x + x), y=int(g.y + y)); xd.sync(); time.sleep(.2)
        def click(x, y):
            motion(x, y)
            for event in (X.ButtonPress, X.ButtonRelease):
                xtest.fake_input(xd, event, 1); xd.sync(); time.sleep(.08)
        def key(name):
            code = xd.keysym_to_keycode(XK.string_to_keysym(name))
            for event in (X.KeyPress, X.KeyRelease):
                xtest.fake_input(xd, event, code); xd.sync(); time.sleep(.08)
        def window():
            return next(w for w in scene()["windows"] if w["id"] == aid)
        def popups():
            return [p for p in scene()["popups"] if p["root"] == aid]
        def main_popup():
            return max(popups(), key=lambda p: p["anchor"]["w"] * p["anchor"]["h"], default=None)
        def shot(name):
            g = host.get_geometry()
            raw = host.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xffffffff)
            Image.frombytes("RGB", (g.width, g.height), raw.data, "raw", "BGRX").save(out / (name + ".png"))
            (out / (name + ".json")).write_text(json.dumps(scene(), indent=2))
        def open_menu():
            r = window()["desktop"]
            click(r["x"] + (24 if toolkit == "fltk" else 100),
                  r["y"] + (13 if toolkit == "fltk" else 80))
            p = wait(lambda: (p if (p := main_popup()) and p["anchor"]["h"] >= 64 else None))
            time.sleep(.4)
            return p
        def point(p, row=0):
            a, r = p["anchor"], window()["desktop"]
            x, y = r["x"] + a["x"], r["y"] + a["y"]
            parent = p["parent"]
            while parent != aid:
                ancestor = next(v for v in popups() if v["id"] == parent)
                x += ancestor["anchor"]["x"]; y += ancestor["anchor"]["y"]
                parent = ancestor["parent"]
            return x + a["w"] * .4, y + a["h"] * (row + .5) / 4
        p = open_menu(); shot("desktop-menu")
        r = window()["desktop"]
        check(r["w"] == 480 and r["h"] == 320,
              toolkit + " fixed-size committed geometry matches scene/XR input dimensions")
        outer = next(v for v in popups() if v["parent"] == aid)
        check(abs(outer["anchor"]["x"] - (r["w"] + 8)) < 1,
              toolkit + " accepts compositor side-panel popup placement")
        click(*point(p)); wait(lambda: "activate" in window()["title"])
        check(True, toolkit + " action accepts pointer at compositor placement")
        wait(lambda: not popups())
        p = open_menu(); before = window()["title"]
        click(*point(p, 2)); time.sleep(.3)
        check(window()["title"] == before, toolkit + " disabled item cannot activate")
        key("Escape"); wait(lambda: not popups())
        p = open_menu(); before = window()["title"]
        click(*point(p, 1)); time.sleep(.8); shot("desktop-submenu")
        check(main_popup() is not None and window()["title"] == before,
              toolkit + " clicking a submenu header keeps the popup open")
        # A pointer leave is not a dismissal gesture, including leaving the
        # entire application surface. Return to the same menu afterwards.
        motion(1550, 920); time.sleep(.8)
        check(main_popup() is not None and window()["title"] == before,
              toolkit + " pointer leaving popup and application does not dismiss the menu")
        motion(*point(p, 1)); time.sleep(.8)
        child = wait(lambda: next((v for v in popups() if v["id"] != p["id"] and v["parent"] == p["id"]), None))
        # point() accounts for title/menu/submenu chains in FLTK.
        cx, cy = point(child)
        click(cx, cy + child["anchor"]["h"] * .375)
        wait(lambda: "nested" in window()["title"])
        check(True, toolkit + " nested menu traverses and activates at configured coordinates")
        wait(lambda: not popups())
        p = open_menu(); before = window()["title"]
        click(*point(p, 3)); wait(lambda: window()["title"] != before)
        check("toggle" in window()["title"], toolkit + " checkable menu item dispatches")
        wait(lambda: not popups())
        p = open_menu(); before = window()["title"]
        key("Down"); key("Return"); wait(lambda: window()["title"] != before)
        check("activate" in window()["title"], toolkit + " keyboard navigation activates native menu")
        wait(lambda: not popups())
        p = open_menu(); key("Escape"); wait(lambda: not popups())
        check(True, toolkit + " Escape dismisses popup family")
        p = open_menu(); click(20, 18); wait(lambda: not popups()); time.sleep(.6)
        check(not popups(), toolkit + " compositor dismissal does not reopen the native menu")
        shot("shell-priority")
        check(True, toolkit + " native menu yields to DomeD shell menu")
        key("Escape")
        post("/api/action", {"op": "focus", "id": aid})
        def heartbeat():
            while not stop.is_set():
                try:
                    post("/api/xr-session", {"visible": True, "focused": True,
                         "head": {"position_m": [0, 1.4, 0], "orientation_xyzw": [0, 0, 0, 1]}, "horizontal_fov_deg": 100})
                except OSError:
                    return
                stop.wait(.1)
        pulse = threading.Thread(target=heartbeat, daemon=True); pulse.start()
        wait(lambda: get("/api/xr-control")["input_realm"] == "xr")
        def xr_click(id, uv):
            post("/api/xr-input", {"events": [{"kind": "motion", "id": id, "uv": uv}], "actions": []})
            time.sleep(.2)
            post("/api/xr-input", {"events": [{"kind": "button", "button": 272, "pressed": True},
                 {"kind": "button", "button": 272, "pressed": False}], "actions": []})
            time.sleep(.3)
        post("/api/xr-input", {"events": [], "actions": [{"op": "focus", "id": aid}]})
        r = window()["desktop"]
        xr_click(aid, [(24 if toolkit == "fltk" else 100) / r["w"], (13 if toolkit == "fltk" else 80) / r["h"]])
        p = wait(lambda: (p if (p := main_popup()) and p["anchor"]["h"] >= 64 else None))
        time.sleep(.6)
        check(p["anchor"]["y"] < 0, toolkit + " accepts above-parent popup configure in XR")
        xr_click(p["id"], [.4, .125]); wait(lambda: "activate" in window()["title"])
        check(True, toolkit + " floating popup accepts XR pointer action")
        wait(lambda: not popups())
        def xr_open_menu():
            xr_click(aid, [(24 if toolkit == "fltk" else 100) / r["w"], (13 if toolkit == "fltk" else 80) / r["h"]])
            p = wait(lambda: (p if (p := main_popup()) and p["anchor"]["h"] >= 64 else None))
            time.sleep(.6)
            return p
        p = xr_open_menu(); before = window()["title"]
        xr_click(p["id"], [.4, .375])
        time.sleep(.6)
        check(main_popup() is not None and window()["title"] == before,
              toolkit + " XR submenu header click keeps the menu open")
        post("/api/xr-input", {"events": [{"kind": "motion", "id": aid, "uv": [.7, .9]}], "actions": []})
        time.sleep(.8)
        check(main_popup() is not None and window()["title"] == before,
              toolkit + " XR pointer leaving popup keeps the menu open")
        for uv in ([.35, .375], [.4, .375]):
            post("/api/xr-input", {"events": [{"kind": "motion", "id": p["id"], "uv": uv}], "actions": []})
            time.sleep(.2)
        child = wait(lambda: next((v for v in popups() if v["parent"] == p["id"]), None))
        time.sleep(.6)
        xr_click(child["id"], [.4, .5]); wait(lambda: "nested" in window()["title"])
        check(True, toolkit + " nested floating menu accepts XR traversal and action")
        wait(lambda: not popups())
        p = xr_open_menu()
        layout = get("/api/xr-shell")
        xr_click(0, [20 / layout["width"], 18 / layout["top"]])
        wait(lambda: not popups()); time.sleep(.6)
        check(not popups(), toolkit + " XR shell dismisses native menus without reopening")
        check(client.poll() is None, toolkit + " stays alive after popup/focus realm changes")
        xd.close()
except Exception as error:
    checks.append({"passed": False, "check": str(error)})
    raise
finally:
    stop.set()
    if pulse:
        pulse.join(timeout=3)
    for process in reversed(processes):
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    for process in reversed(processes):
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL); process.wait()
    for log in logs:
        log.close()
    (out / "results.json").write_text(json.dumps(checks, indent=2) + "\n")
