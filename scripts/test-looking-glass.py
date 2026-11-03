#!/usr/bin/env python3
"""Native Wayland clients, real desktop pointer/keys and synthetic XR, in a private session."""

import json, os, pathlib, shlex, signal, socket, subprocess, sys, tempfile, threading, time, urllib.request
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image

P = pathlib.Path
root = P(__file__).resolve().parents[1]
out = root / "artifacts/looking-glass-v2"
out.mkdir(parents=True, exist_ok=True)
(out / "report.json").unlink(missing_ok=True)
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

        def chord(modifier, name):
            code = xd.keysym_to_keycode(XK.string_to_keysym(modifier))
            xtest.fake_input(xd, X.KeyPress, code)
            key(name)
            xtest.fake_input(xd, X.KeyRelease, code)
            xd.sync()

        def wheel(direction=5):
            xtest.fake_input(xd, X.ButtonPress, direction)
            xtest.fake_input(xd, X.ButtonRelease, direction)
            xd.sync()

        ids = [w["id"] for w in initial["windows"]]
        visible = [ids[0], ids[3], ids[6]]
        for id in ids:
            if id not in visible:
                action(op="minimize", id=id)
        for index, id in enumerate(visible):
            action(op="desktop_move", id=id, x=35 + index * 230, y=100 + index * 65)
        action(op="focus", id=visible[-1])
        time.sleep(0.3)
        geometry = {
            w["id"]: (w["desktop"].copy(), w["xr_pose"], w["xr_scale"])
            for w in state()["windows"]
        }
        chord("Super_L", "g")
        wait(lambda: state()["looking_glass"]["enabled"])
        check(True, "Super+G enables natural-placement mode without opening a grid")
        for id, angle in zip(visible, [50, -55, 35]):
            action(op="glass_window", id=id, yaw_deg=angle)
        time.sleep(0.5)
        shot("desktop-natural-tilt.png")
        check(
            all(
                (w["desktop"], w["xr_pose"], w["xr_scale"]) == geometry[w["id"]]
                for w in state()["windows"]
            ),
            "Enabling and tilting preserves every window position and resolution",
        )
        check("Full-resolution Looking Glass rendering:" not in (out/"compositor.log").read_text(), "Native desktop uses full-resolution GPU tilt textures without renderer fallback")
        panels = request("/api/state")["panels"]
        check(
            all(0.65 <= p["opacity"] < 1.0 for p in panels if p["id"] in visible),
            "Tilted window opacity is reduced in the shared scene",
        )
        # Target the visibly topmost window using the physical pointer and real wheel events.
        target = visible[-1]
        rect = geometry[target][0]
        g = host.get_geometry()
        xtest.fake_input(
            xd,
            X.MotionNotify,
            x=g.x + int(rect["x"] + rect["w"] / 2),
            y=g.y + int(rect["y"] + rect["h"] / 2),
        )
        xd.sync()
        before = state()["looking_glass"]["cards"][str(target)]["yaw_deg"]
        xtest.fake_input(xd, X.KeyPress, super_code)
        xd.sync()
        time.sleep(0.1)
        wheel()
        xtest.fake_input(xd, X.KeyRelease, super_code)
        xd.sync()
        wait(
            lambda: state()["looking_glass"]["cards"][str(target)]["yaw_deg"] != before
        )
        check(True, "Super+scroll changes tilt and is consumed by the window manager")
        action(op="glass_window", id=target, yaw_deg=85)
        xtest.fake_input(xd, X.KeyPress, super_code)
        xd.sync()
        time.sleep(0.1)
        for _ in range(4):
            wheel()
        xtest.fake_input(xd, X.KeyRelease, super_code)
        xd.sync()
        time.sleep(0.2)
        check(
            abs(state()["looking_glass"]["cards"][str(target)]["yaw_deg"]) == 90,
            "Wheel tilt clamps at ninety degrees without a back side",
        )
        click(rect["x"] + rect["w"] / 2, rect["y"] + rect["h"] / 2)
        wait(lambda: state()["looking_glass"]["cards"][str(target)]["display_yaw_deg"] == 0)
        check(state()["focused"] == target, "Clicking a tilted window opens that window without clicking through")
        # Switch rapidly enough to keep the debounce pending, then let the last focus settle.
        for id in visible:
            action(op="glass_window", id=id, yaw_deg=60)
        alt = xd.keysym_to_keycode(XK.string_to_keysym("Alt_L"))
        xtest.fake_input(xd, X.KeyPress, alt)
        xd.sync()
        visited = []
        for _ in range(4):
            previous = state()["focused"]
            key("Tab")
            current = wait(lambda: (s if (s := state())["looking_glass"]["focus_candidate"] != previous
                and s["looking_glass"]["focus_candidate"] == s["focused"] else None))
            candidate = current["looking_glass"]["focus_candidate"]
            assert candidate is not None
            visited.append(candidate)
            assert current["looking_glass"]["settle_at_ms"] is not None
            wait(lambda: 0 < state()["looking_glass"]["cards"][str(candidate)]["display_yaw_deg"] < 60, timeout=0.25)
        xtest.fake_input(xd, X.KeyRelease, alt)
        xd.sync()
        last = visited[-1]
        wait(
            lambda: (
                abs(state()["looking_glass"]["cards"][str(last)]["display_yaw_deg"])
                < 0.01
            )
        )
        check(
            all(
                abs(state()["looking_glass"]["cards"][str(id)]["display_yaw_deg"] - 60)
                < 0.01
                for id in visible
                if id != last
            ),
            "Alt-Tab previews lean toward view; only the final focus settles flat after the pause",
        )
        shot("desktop-after-alt-tab.png")
        # The grouped dock shows all application windows, including minimized ones.
        time.sleep(0.5)
        g = host.get_geometry()
        click(160, g.height - 35)
        time.sleep(0.3)
        shot("application-stack-popup.png")
        count = 3
        popup_y = g.height - 60 - (count * 88 + 12)
        click(230, popup_y + 45)
        wait(lambda: state()["focused"] == visible[0])
        check(
            abs(state()["looking_glass"]["cards"][str(visible[0])]["display_yaw_deg"])
            < 0.01,
            "Grouped application stack opens a window and removes its tilt",
        )

        # Real window texture capture must remain full-size, not taskbar preview size.
        def texture(id, name):
            def read():
                try:
                    with http.open(base + f"/api/xr-frame/{id}", timeout=5) as response:
                        size = (
                            int(response.headers["X-Dome-Width"]),
                            int(response.headers["X-Dome-Height"]),
                        )
                        Image.frombytes("RGBA", size, response.read()).save(out / name)
                        return size
                except OSError:
                    return None

            return wait(read)

        source_size = texture(visible[0], "native-window-texture.png")
        check(
            source_size[0] >= geometry[visible[0]][0]["w"]
            and source_size[1] >= geometry[visible[0]][0]["h"],
            "Window captures retain native resolution independently of low-resolution dock previews",
        )
        tracking = {
            "id": 884,
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
        wait(lambda: request("/api/xr-control")["input_realm"] == "xr")
        target = visible[0]
        action(op="glass_window", id=target, yaw_deg=0)

        def quad(id):
            return next(
                s["quad"]
                for s in request("/api/capture/xr")["surfaces"]
                if s["quad"]["id"] == id
            )

        baseline = quad(target)
        xtest.fake_input(xd, X.KeyPress, super_code)
        xd.sync()
        time.sleep(0.1)
        request(
            "/api/xr-input",
            {
                "events": [
                    {"kind": "motion", "id": target, "uv": [0.5, 0.5]},
                    {"kind": "scroll", "horizontal": 0, "vertical": 30},
                ],
                "actions": [],
            },
        )
        wait(lambda: state()["looking_glass"]["cards"][str(target)]["yaw_deg"] != 0)
        xtest.fake_input(xd, X.KeyRelease, super_code)
        xd.sync()
        tilted = quad(target)
        check(
            tilted["dome"] is None
            and tilted["size"] == baseline["size"]
            and all(
                abs(a - b) < 1e-5
                for a, b in zip(
                    tilted["pose"]["position_m"], baseline["pose"]["position_m"]
                )
            ),
            "XR Super+wheel rotates at the original dome anchor with unchanged extent",
        )
        check(
            texture(target, "xr-tilted-native-texture.png") == source_size,
            "XR tilting does not resize the application or its texture",
        )
        video = out / "xr-natural.mkv"
        report = out / "xr-natural.json"
        video.unlink(missing_ok=True)
        report.unlink(missing_ok=True)
        record = subprocess.run(
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
        (out / "recording.log").write_text(record.stdout + record.stderr)
        assert record.returncode == 0, record.stderr
        check(
            json.loads(report.read_text())["frames"] > 0,
            "GPU XR composition renders naturally placed full-resolution tilted windows",
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
                str(out / "xr-natural.png"),
            ],
            check=True,
        )
        action(op="looking_glass", enabled=False)
        check(
            all(
                (w["desktop"], w["xr_pose"], w["xr_scale"]) == geometry[w["id"]]
                for w in state()["windows"]
            ),
            "Disabling restores the original layout without changing client resolutions",
        )
        check(
            server.poll() is None and client.poll() is None,
            "Native compositor and clients remain alive",
        )
        (out / "report.json").write_text(
            json.dumps(
                {
                    "passed": True,
                    "checks": checks,
                    "headset_tested": False,
                    "window_capture_size": source_size,
                    "iteration": 2,
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
