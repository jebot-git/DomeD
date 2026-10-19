#!/usr/bin/env python3
"""Real KMS/page-flip/EGL tests on the private VKMS card; never opens physical input.
Prepare: pkexec /usr/bin/bash scripts/vkms-test-device.sh prepare
Run: uv run --no-project --with PySide6 --with pillow python scripts/test-native-drm.py [--installed]
"""
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request

import PySide6
from PIL import Image

if os.environ.get("DOME_DRM_TEST_BUS") != "1":
    os.execvpe("dbus-run-session", ["dbus-run-session", "--", sys.executable, *sys.argv],
              {**os.environ, "DOME_DRM_TEST_BUS": "1"})
root = Path(__file__).resolve().parents[1]
installed = "--installed" in sys.argv
multi_gpu = "--multi-gpu" in sys.argv
out = root / "artifacts" / ("native-multi-gpu" if multi_gpu else "native-installed" if installed else "native-drm")
out.mkdir(parents=True, exist_ok=True)
checks, processes, logs = [], [], []
pulse_stop = threading.Event()


def check(ok, label):
    checks.append({"check": label, "passed": bool(ok)})
    print(("PASS " if ok else "FAIL ") + label, flush=True)
    assert ok, label


def wait(predicate):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        try:
            value = predicate()
            if value:
                return value
        except (OSError, ValueError, KeyError):
            pass
        time.sleep(0.05)
    raise AssertionError("Timed out; see " + str(out))


def start(args, name, env):
    log = (out / (name + ".log")).open("w")
    logs.append(log)
    child = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT)
    processes.append(child)
    return child


with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
base = f"http://127.0.0.1:{port}"


def get(path):
    with urllib.request.urlopen(base + path, timeout=2) as response:
        return json.load(response)


def post(path, value):
    request = urllib.request.Request(base + path, data=json.dumps(value).encode(),
                                     headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=2) as response:
        return json.load(response)


def state():
    return get("/api/state")["scene"]


def report():
    if not multi_gpu: return json.loads((out / "scanout.json").read_text())
    cards = sorted([*Path("/sys/devices/faux/dome-test/drm").glob("card[0-9]*"),*Path("/sys/devices/faux/dome-test-gpu2/drm").glob("card[0-9]*")])
    reports=[json.loads((out/("scanout-"+card.name+".json")).read_text()) for card in cards]
    return {**reports[0],"outputs":[o for r in reports for o in r["outputs"]],
        "presented_frames":sum(r["presented_frames"] for r in reports),"rendered_frames":sum(r["rendered_frames"] for r in reports),"active":all(r["active"] for r in reports)}


def events():
    return [json.loads(line) for line in (out / "client.jsonl").read_text().splitlines()]


def frame_path(index=0):
    return out / ("desktop-" + report()["outputs"][index]["name"] + ".ppm")


def pixel_matches(point, color, index=0):
    with Image.open(frame_path(index)) as frame:
        return all(abs(a-b) < 8 for a, b in zip(frame.getpixel(point), color))


result = 1
try:
    with tempfile.TemporaryDirectory(prefix="dome-native-drm-") as directory:
        runtime = Path(directory)
        runtime.chmod(0o700)
        cards = list(Path("/sys/devices/faux/dome-test/drm").glob("card[0-9]*"))
        assert cards, "Prepare the private VKMS test output first"
        device = os.environ.get("DOME_DRM_TEST_DEVICE", "/dev/dri/" + cards[0].name)
        for name in ("scanout.json", "desktop.ppm", "client.jsonl"):
            (out / name).unlink(missing_ok=True)
        env = {**os.environ, "XDG_RUNTIME_DIR": directory,
               "XDG_CONFIG_HOME": str(runtime / "config"), "XDG_STATE_HOME": str(runtime / "state"),
               "DOME_DRM_TEST_DEVICE": device, "DOME_DRM_REPORT": str(out / "scanout.json"),
               "DOME_DRM_CAPTURE": str(out / "desktop.ppm"), "DOME_DRM_CAPTURE_ALL": "1", "DOME_DRM_TEST_CONTROL": str(runtime / "control.sock"),
               "DOME_START_XR": "0", "DOME_SESSION_TERMINAL": "0", "DOME_PREVIEW_PORT": str(port),
               "DOME_SESSION_LOG": str(out / "session.log"), "LIBGL_ALWAYS_SOFTWARE": "1", "RUST_LOG": "warn"}
        for name in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "QT_QPA_PLATFORM", "QT_PLUGIN_PATH", "QT_QPA_PLATFORM_PLUGIN_PATH"):
            env.pop(name, None)
        if multi_gpu:
            other=list(Path("/sys/devices/faux/dome-test-gpu2/drm").glob("card[0-9]*"))
            assert other, "Prepare second test GPU first"
            env["DOME_DRM_TEST_DEVICES"]=device+",/dev/dri/"+other[0].name
            env["DOME_DRM_REPORT_ALL"]="1"
            for report_file in out.glob("scanout-*.json"): report_file.unlink()
        (out / "session.log").write_text("")
        args = ["/usr/local/bin/dome-session"] if installed else [str(root / "target/debug/dome-compositor"), "--backend", "drm"]
        server = start(args, "compositor", env)
        wait(lambda: state()["source"] == "wayland")
        wait(lambda: report()["presented_frames"] > 3)
        check(report()["physical_scanout"] and report()["backend"] == "drm", "Dome owns real virtual-DRM scanout without X11, Wayland host or KWin")
        wait(lambda: len(report()["outputs"]) == (3 if multi_gpu else 2) and all(o["presented_frames"]>3 for o in report()["outputs"]))
        check(True, "Two connectors use independent CRTCs and page-flip queues")
        check(report()["outputs"][1]["x"] == report()["outputs"][0]["width"], "Monitors form an extended desktop rather than overlapping")
        log = (out / ("session.log" if installed else "compositor.log")).read_text()
        wayland_socket = re.search(r"Dome Wayland socket: (\S+)", log)[1]
        client_env = {**env, "WAYLAND_DISPLAY": wayland_socket, "QT_QPA_PLATFORM": "wayland",
                      "QT_WAYLAND_CLIENT_BUFFER_INTEGRATION": "shm",
                      "LD_LIBRARY_PATH": str(Path(PySide6.__file__).parent / "Qt/lib")}
        start([sys.executable, str(root / "scripts/desktop-client.py"), "--name", "Native DRM client",
               "--color", "#36a381", "--journal", str(out / "client.jsonl")], "client", client_env)
        wait(lambda: len(state()["windows"]) == 1)
        window = state()["windows"][0]
        point = (int(window["desktop"]["x"]+100), int(window["desktop"]["y"]+100))
        wait(lambda: pixel_matches(point, (54, 163, 129)))
        check(True, "Native Wayland client pixels reach the DRM framebuffer")
        with Image.open(frame_path()) as frame:
            frame.save(out / "desktop.png")
        old = report()["presented_frames"]
        wait(lambda: report()["presented_frames"] > old + 3)
        check(True, "Page-flip events retire scanout buffers and keep frames advancing")

        second_index=len(report()["outputs"])-1
        second = report()["outputs"][second_index]
        post("/api/action", {"op":"desktop_move","id":window["id"],"x":second["x"]+80,"y":120})
        wait(lambda: pixel_matches((180,220),(54,163,129),second_index))
        check(True, "A window moved onto the second monitor renders at output-local coordinates")
        post("/api/action", {"op":"maximize","id":window["id"]})
        wait(lambda: pixel_matches((second["width"]-50,second["height"]-100),(54,163,129),second_index))
        check(not pixel_matches((500,400),(54,163,129),0), "Maximization stays on the window's monitor and reserves its bars")
        post("/api/action", {"op":"maximize","id":window["id"]})
        post("/api/action", {"op":"desktop_move","id":window["id"],"x":80,"y":120})

        # Apply saved layout preferences through the same action used by the settings UI.
        config = state()["config"]
        config["displays"] = [{"name":second["name"],"enabled":True,"width":0,"height":0,"refresh":0,"x":second["x"],"y":-200,"primary":True}]
        post("/api/action", {"op":"configure","config":config})
        wait(lambda: report()["outputs"][second_index]["y"] == -200)
        post("/api/action", {"op":"desktop_move","id":window["id"],"x":second["x"]+80,"y":-100})
        wait(lambda:pixel_matches((180,200),(54,163,129),second_index))
        check(next(w for w in state()["windows"] if w["id"]==window["id"])["desktop"]["y"]==-100,"Windows render on monitors above the desktop origin")
        check(True, "Display configuration supports negative desktop coordinates")
        config["displays"] = []
        post("/api/action", {"op":"configure","config":config})
        wait(lambda: report()["outputs"][1]["x"] == report()["outputs"][0]["width"])

        post("/api/action", {"op":"desktop_move","id":window["id"],"x":80,"y":120})
        if not multi_gpu:
            # Simulate unplug/replug on a private virtual connector only.
            def connector_change(command):
                subprocess.run(["pkexec","/usr/bin/bash",str(root/"scripts/vkms-test-device.sh"),command],check=True,timeout=60)
            with socket.socket(socket.AF_UNIX,socket.SOCK_DGRAM) as control:
                connector_change("disconnect-second")
                control.sendto(b"rescan",str(runtime/"control.sock"))
                wait(lambda:len(report()["outputs"])==1)
                old_presented=report()["presented_frames"]
                wait(lambda:report()["presented_frames"]>old_presented+2)
                check(True,"Unplugging one monitor keeps the remaining scanout alive")
                connector_change("connect-second")
                control.sendto(b"rescan",str(runtime/"control.sock"))
                wait(lambda:len(report()["outputs"])==2 and all(o["presented_frames"]>2 for o in report()["outputs"]))
                check(True,"Replugging restores an independent scanout without restarting the compositor")

        def heartbeat():
            while not pulse_stop.is_set():
                try:
                    post("/api/xr-session", {"id": 700, "visible": True, "focused": True,
                         "head": {"position_m": [0, 1.4, 0], "orientation_xyzw": [0, 0, 0, 1]}, "horizontal_fov_deg": 100})
                except OSError:
                    pass
                pulse_stop.wait(0.1)
        pulse = threading.Thread(target=heartbeat, daemon=True)
        pulse.start()
        wait(lambda: get("/api/xr-control")["input_realm"] == "xr")
        post("/api/xr-input", {"actions": [], "events": [{"kind": "motion", "id": window["id"], "uv": [0.5, 0.5]},
              {"kind": "button", "button": 0x110, "pressed": True}]})
        wait(lambda: any(e["kind"] == "press" for e in events()))
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as control:
            control.sendto(b"pause", str(runtime / "control.sock"))
            wait(lambda: not report()["active"])
            wait(lambda: any(e["kind"] == "release" for e in events()))
            rendered = report()["rendered_frames"]
            time.sleep(0.2)
            check(report()["rendered_frames"] == rendered, "Seat pause stops scanout rendering")
            check(sum(e["kind"] == "release" for e in events()) == 1, "Seat pause balances a held client pointer button exactly once")
            post("/api/xr-input", {"actions": [], "events": [{"kind": "button", "button": 0x110, "pressed": True}]})
            time.sleep(0.15)
            check(sum(e["kind"] == "press" for e in events()) == 1, "Inactive seat rejects queued XR input")
            control.sendto(b"resume", str(runtime / "control.sock"))
            wait(lambda: report()["active"] and report()["presented_frames"] > old + 6)
            check(True, "Seat resume resets buffers and restores actual KMS page flips")
        pulse_stop.set()
        pulse.join(timeout=2)
        wait(lambda: not get("/api/xr-control")["visible"])

        egl_env = {**client_env, "QT_WAYLAND_CLIENT_BUFFER_INTEGRATION": "wayland-egl"}
        egl = start([sys.executable, str(root / "scripts/egl-client.py")], "egl-client", egl_env)
        wait(lambda: len(state()["windows"]) == 2)
        egl_window = next(w for w in state()["windows"] if w["title"] == "Dome EGL fixture")
        point = (int(egl_window["desktop"]["x"]+100), int(egl_window["desktop"]["y"]+100))
        wait(lambda: pixel_matches(point, (51, 76, 230)))
        check(egl.poll() is None, "Native EGL client renders into DRM scanout")
        with Image.open(frame_path()) as frame:
            frame.save(out / "egl.png")
        if multi_gpu:
            post("/api/action", {"op":"desktop_move","id":egl_window["id"],"x":second["x"]+80,"y":120})
            wait(lambda:pixel_matches((180,220),(51,76,230),second_index))
            check(True,"An EGL client can move between GPU scanout devices")
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as control:
            control.sendto(b"quit", str(runtime / "control.sock"))
        check(server.wait(timeout=10) == 0, "Logout exits the native compositor and releases DRM ownership cleanly")
        result = 0
except Exception as error:
    print("NATIVE DRM TEST FAILED:", repr(error), flush=True)
    checks.append({"check": str(error), "passed": False})
finally:
    pulse_stop.set()
    for child in reversed(processes):
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
    for log in logs:
        log.close()
    (out / "report.json").write_text(json.dumps({"checks": checks, "failed": sum(not c["passed"] for c in checks)}, indent=2))
sys.exit(result)
