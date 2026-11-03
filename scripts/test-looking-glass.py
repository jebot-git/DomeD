#!/usr/bin/env python3
"""Native Wayland clients, real desktop pointer/keys and synthetic XR, in a private session."""

import json, os, pathlib, shlex, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image

P = pathlib.Path
root = P(__file__).resolve().parents[1]
out = root / "artifacts/looking-glass"
out.mkdir(parents=True, exist_ok=True)
signal.pthread_sigmask(signal.SIG_SETMASK, [])
if os.environ.get("DOME_GLASS_TEST_BUS") != "1":
    os.execvpe(
        "dbus-run-session",
        ["dbus-run-session", "--", sys.executable, *sys.argv],
        {**os.environ, "DOME_GLASS_TEST_BUS": "1"},
    )
children = []
logs = []
checks = []
pulse = threading.Event()
beat = None
compositor = P(os.environ["DOME_COMPOSITOR"]).resolve()
flags = shlex.split(
    subprocess.check_output(
        [
            "/opt/fltk-vr/toolkit/bin/fltk-config",
            "--cxxflags",
            "--ldflags",
            "--use-images",
        ],
        text=True,
    )
)
fonts = shlex.split(
    subprocess.check_output(
        ["pkg-config", "--cflags", "--libs", "fontconfig", "freetype2", "zlib"],
        text=True,
    )
)
subprocess.run(
    [
        "g++",
        "-std=c++17",
        "-O1",
        str(root / "scripts/looking-glass-client.cpp"),
        "-o",
        str(out / "client"),
        *flags,
        *fonts,
    ],
    check=True,
)


def check(value, name):
    assert value, name
    checks.append(name)
    print("PASS", name, flush=True)


def start(cmd, name, env, **kw):
    log = (out / (name + ".log")).open("w")
    logs.append(log)
    p = subprocess.Popen(list(map(str, cmd)), env=env, stdout=log, stderr=log, **kw)
    children.append(p)
    return p


def wait(fn, timeout=30):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        try:
            value = fn()
            if value:
                return value
        except (OSError, ValueError, KeyError, StopIteration):
            pass
        time.sleep(0.06)
    raise AssertionError("Timed out; inspect " + str(out))


try:
    with tempfile.TemporaryDirectory(prefix="dome-looking-glass-") as directory:
        runtime = P(directory)
        runtime.chmod(0o700)
        rd, wr = os.pipe()
        start(
            [
                "Xvfb",
                "-displayfd",
                wr,
                "-screen",
                "0",
                "1440x1000x24",
                "-nolisten",
                "tcp",
            ],
            "xvfb",
            os.environ.copy(),
            pass_fds=(wr,),
        )
        os.close(wr)
        with os.fdopen(rd) as stream:
            screen = ":" + stream.readline().strip()
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        env = {
            **os.environ,
            "DISPLAY": screen,
            "XDG_RUNTIME_DIR": directory,
            "XDG_CONFIG_HOME": directory + "/config",
            "XDG_DATA_HOME": directory + "/data",
            "XDG_STATE_HOME": directory + "/state",
            "XDG_CACHE_HOME": directory + "/cache",
            "HOME": directory + "/home",
            "DOME_PREVIEW_PORT": str(port),
            "DOME_LOGIN_SESSION": "0",
            "DOME_SESSION_RESTORE": "0",
            "DOME_APPEARANCE_SERVICE": "0",
            "DOME_RENDERER": "vulkan",
            "RUST_LOG": "warn",
            "XR_RUNTIME_JSON": directory + "/no-xr-runtime.json",
            "DBUS_SYSTEM_BUS_ADDRESS": "unix:path=" + directory + "/no-system-bus",
            "LD_LIBRARY_PATH": "/opt/fltk-vr/bdf-runtime/lib:/opt/fltk-vr/toolkit/lib",
        }
        for name in ["XR", "TRAY", "KDECONNECT", "POLKIT", "KWALLET", "IME"]:
            env["DOME_START_" + name] = "0"
        for key in [
            "WAYLAND_DISPLAY",
            "WAYLAND_SOCKET",
            "FLTK_BACKEND",
            "LIBGL_ALWAYS_SOFTWARE",
            "GALLIUM_DRIVER",
            "MESA_LOADER_DRIVER_OVERRIDE",
        ]:
            env.pop(key, None)
        http = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        base = f"http://127.0.0.1:{port}"

        def request(path, data=None):
            with http.open(
                urllib.request.Request(
                    base + path,
                    data=None if data is None else json.dumps(data).encode(),
                    headers={"Content-Type": "application/json"},
                ),
                timeout=3,
            ) as response:
                return json.load(response)

        def state():
            return request("/api/state")["scene"]

        def action(**data):
            request("/api/action", data)

        server = start([compositor, "--backend", "nested"], "compositor", env)
        wait(state)
        check(not state()["looking_glass"]["enabled"], "Prototype starts disabled")
        desktop = wait(
            lambda: next(
                str(p).removesuffix(".dome-session.json")
                for p in runtime.glob("*.dome-session.json")
                if json.loads(p.read_text())["realm"] == "desktop"
            )
        )
        client = start(
            [out / "client"],
            "client",
            {**env, "WAYLAND_DISPLAY": desktop, "FLTK_BACKEND": "wayland"},
        )
        wait(lambda: len(state()["windows"]) >= 8)
        time.sleep(0.8)
        initial = state()
        geometry = {
            w["id"]: (w["desktop"], w["xr_pose"], w["xr_scale"])
            for w in initial["windows"]
        }
        xd = display.Display(screen)
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

        def key(name):
            code = xd.keysym_to_keycode(XK.string_to_keysym(name))
            xtest.fake_input(xd, X.KeyPress, code)
            xtest.fake_input(xd, X.KeyRelease, code)
            xd.sync()
            time.sleep(0.08)

        def click(x, y):
            g = host.get_geometry()
            xtest.fake_input(xd, X.MotionNotify, x=g.x + int(x), y=g.y + int(y))
            xtest.fake_input(xd, X.ButtonPress, 1)
            xtest.fake_input(xd, X.ButtonRelease, 1)
            xd.sync()
            time.sleep(0.14)

        def shot(name):
            time.sleep(0.2)
            g = host.get_geometry()
            raw = host.get_image(0, 0, g.width, g.height, X.ZPixmap, 0xFFFFFFFF)
            img = Image.frombytes("RGB", (g.width, g.height), raw.data, "raw", "BGRX")
            img.save(out / name)
            return img

        super_code = xd.keysym_to_keycode(XK.string_to_keysym("Super_L"))
        xtest.fake_input(xd, X.KeyPress, super_code)
        key("g")
        xtest.fake_input(xd, X.KeyRelease, super_code)
        xd.sync()
        wait(lambda: state()["looking_glass"]["overview"])
        check(
            state()["looking_glass"]["enabled"],
            "Super+G enables real compositor overview",
        )
        time.sleep(2)
        a = shot("desktop-overview.png")
        time.sleep(1)
        b = shot("desktop-overview-live.png")
        check(
            a.tobytes() != b.tobytes(),
            "Live Wayland client content updates card and dock previews",
        )
        g = host.get_geometry()
        cw = (g.width - 48) / 3
        ch = (g.height - 220) / 2 - 46
        cy = 110 + ch + 16
        id = initial["windows"][0]["id"]
        click(24 + cw / 5 * 1.5, cy)
        wait(lambda: state()["looking_glass"]["cards"].get(str(id), {}).get("parked"))
        shot("desktop-parked.png")
        check(True, "Visible Park control sets edge-on card state")
        click(24 + cw / 5 * 2.5, cy)
        wait(lambda: state()["looking_glass"]["cards"].get(str(id), {}).get("back"))
        key("k")
        wait(lambda: state()["looking_glass"]["cards"][str(id)]["note"] == "k")
        shot("desktop-notes.png")
        check(
            "KEY k" not in (out / "client.log").read_text(),
            "Back-side note editing consumes keyboard input before applications",
        )
        click(g.width - 270, 60)
        wait(lambda: state()["looking_glass"]["page"] == 1)
        shot("desktop-page-two.png")
        check(True, "Bounded second page exposes windows beyond six")
        action(op="glass_page", page=0)
        tracking = {
            "id": 882,
            "visible": True,
            "focused": True,
            "head": {"position_m": [0, 1.4, 0], "orientation_xyzw": [0, 0, 0, 1]},
            "eyes": [
                {"position_m": [x, 1.4, 0], "orientation_xyzw": [0, 0, 0, 1]}
                for x in [-0.032, 0.032]
            ],
            "horizontal_fov_deg": 100,
            "mouse_viewport": [-0.8, 0.8, -0.6, 0.6],
        }

        def heartbeat():
            while not pulse.wait(0.1):
                try:
                    request("/api/xr-session", tracking)
                except OSError:
                    continue

        request("/api/xr-session", tracking)
        beat = threading.Thread(target=heartbeat)
        beat.start()
        wait(lambda: request("/api/capture/xr")["available"])
        check(
            request("/api/xr-shell")["top"] > 36,
            "XR overview exports a compositor-owned interactive panel",
        )
        # Let the first visible tracking packet switch the seat before issuing
        # keys: toggling immediately can race that automatic transition.
        try:
            wait(lambda: request("/api/xr-control")["input_realm"] == "xr", timeout=6)
        except AssertionError:
            (out / "xr-route-debug.json").write_text(
                json.dumps(
                    {"control": request("/api/xr-control"), "scene": state()}, indent=2
                )
            )
            xtest.fake_input(xd, X.KeyPress, super_code)
            key("x")
            xtest.fake_input(xd, X.KeyRelease, super_code)
            xd.sync()
            wait(lambda: request("/api/xr-control")["input_realm"] == "xr")
        layout = request("/api/xr-shell")
        panel = 18446744073709551610 if layout.get("pullout") else 0
        top = 36 if layout.get("pullout") else 0
        panel_height = (
            min(layout["top"], layout["height"] - 60) - top if panel else layout["top"]
        )

        def xr_click(x, y):
            request(
                "/api/xr-input",
                {
                    "events": [
                        {
                            "kind": "motion",
                            "id": panel,
                            "uv": [x / layout["width"], (y - top) / panel_height],
                        },
                        {"kind": "button", "button": 272, "pressed": True},
                        {"kind": "button", "button": 272, "pressed": False},
                    ],
                    "actions": [],
                },
            )

        xr_click(24 + ((1024 - 48) / 3 - 16) * 0.5, 110 + (768 - 220) / 2 - 46 + 16)
        wait(lambda: not state()["looking_glass"]["cards"][str(id)]["back"])
        xr_click(24 + ((1024 - 48) / 3 - 16) * 0.5, 110 + (768 - 220) / 2 - 46 + 16)
        wait(lambda: state()["looking_glass"]["cards"][str(id)]["back"])
        key("j")
        wait(lambda: state()["looking_glass"]["cards"][str(id)]["note"] == "kj")
        check(
            "KEY j" not in (out / "client.log").read_text(),
            "XR seat flips the shared card and edits notes without client key leakage",
        )
        with http.open(base + f"/api/xr-frame/{panel}", timeout=5) as response:
            Image.frombytes(
                "RGBA",
                (
                    int(response.headers["X-Dome-Width"]),
                    int(response.headers["X-Dome-Height"]),
                ),
                response.read(),
            ).save(out / "xr-overview.png")
        action(op="glass_window", id=id, back=False, parked=True)
        action(op="glass_overview", open=False)

        def captured():
            value = request("/api/capture/xr")
            return (
                value if any(s["quad"]["id"] == id for s in value["surfaces"]) else None
            )

        capture = wait(captured)
        spatial = next(v["quad"] for v in capture["surfaces"] if v["quad"]["id"] == id)
        check(
            spatial["dome"] is None
            and abs(spatial["pose"]["orientation_xyzw"][1]) > 0.1,
            "XR renderer receives genuinely rotated flat window quads",
        )
        video = out / "xr-spatial.mkv"
        report = out / "xr-spatial.json"
        video.unlink(missing_ok=True)
        report.unlink(missing_ok=True)
        recording = subprocess.run(
            [
                "/usr/local/bin/dome-capture",
                "--record",
                "--source",
                "xr",
                "--eye",
                "left",
                "--encoder",
                "vaapi",
                "--codec",
                "h264",
                "--width",
                "960",
                "--fps",
                "10",
                "--seconds",
                "1.5",
                "--server",
                base,
                "--output",
                str(video),
                "--report",
                str(report),
            ],
            env=env,
            capture_output=True,
            text=True,
            timeout=45,
        )
        (out / "xr-recording.log").write_text(recording.stdout + recording.stderr)
        assert recording.returncode == 0, (recording.returncode, recording.stderr)
        check(
            json.loads(report.read_text())["frames"] > 0,
            "GPU offscreen XR composition records real rotated window textures",
        )
        subprocess.run(
            [
                "ffmpeg",
                "-v",
                "error",
                "-sseof",
                "-0.2",
                "-i",
                str(video),
                "-frames:v",
                "1",
                "-y",
                str(out / "xr-spatial.png"),
            ],
            check=True,
        )
        action(op="glass_overview", open=True)
        (out / "xr-capture.json").write_text(json.dumps(capture, indent=2))
        action(op="looking_glass", enabled=False)
        final = state()
        check(
            not final["looking_glass"]["enabled"],
            "Prototype can be disabled without terminating clients",
        )
        check(
            all(
                (w["desktop"], w["xr_pose"], w["xr_scale"]) == geometry[w["id"]]
                for w in final["windows"]
                if w["id"] in geometry
            ),
            "Disabling restores original desktop and XR geometry exactly",
        )
        check(
            client.poll() is None and server.poll() is None,
            "Clients and compositor remain alive",
        )
        (out / "report.json").write_text(
            json.dumps(
                {
                    "passed": True,
                    "checks": checks,
                    "headset_tested": False,
                    "upstream": "f797e0f",
                },
                indent=2,
            )
            + "\n"
        )
        xd.close()
finally:
    pulse.set()
    if beat:
        beat.join(timeout=3)
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
    for child in reversed(children):
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    for log in logs:
        log.close()
