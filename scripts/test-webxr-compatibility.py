#!/usr/bin/env python3
"""Exercise optional WebXR bridge startup using an isolated OpenXR runtime.

Run under runtime-with-sim-service; this script does not launch or contact a
desktop compositor. XR_RUNTIME_JSON and XDG_RUNTIME_DIR must name that fixture.
"""

import argparse
import json
import os
import socket
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "target/debug/dome-xr",
    )
    parser.add_argument("--artifacts", type=Path, required=True)
    args = parser.parse_args()
    args.artifacts.mkdir(parents=True, exist_ok=True)
    runtime = Path(os.environ["XDG_RUNTIME_DIR"])
    assert os.environ.get("XR_RUNTIME_JSON"), "An isolated OpenXR runtime is required"
    with tempfile.TemporaryDirectory(prefix="webxr-compat-", dir=runtime) as temporary:
        root = Path(temporary)

        def run(name, extra):
            report = args.artifacts / f"{name}.json"
            command = [
                str(args.binary.resolve()),
                "--frames",
                "3",
                "--transport",
                "cpu",
                "--server",
                "http://127.0.0.1:1",
                "--report",
                str(report),
                *extra,
            ]
            result = subprocess.run(
                command, capture_output=True, text=True, timeout=30, check=False
            )
            (args.artifacts / f"{name}.log").write_text(result.stdout + result.stderr)
            assert result.returncode == 0, f"{name}: {result.stderr[-3000:]}"
            summary = json.loads(report.read_text())
            assert summary["clean_exit"] and 3 <= summary["frames"] <= 4, summary
            assert summary["webxr"]["sessions"] == 0, summary
            return result.stderr

        occupied = root / "occupied.sock"
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(occupied))
            listener.listen(1)
            identity = occupied.stat().st_ino
            log = run(
                "occupied-endpoint", ["--standalone", "--webxr-socket", str(occupied)]
            )
            assert "WebXR bridge unavailable" in log, log
            assert occupied.stat().st_ino == identity
            with socket.socket(socket.AF_UNIX) as client:
                client.connect(str(occupied))
                accepted, _ = listener.accept()
                accepted.close()
        occupied.unlink()

        missing = root / "missing" / "bridge.sock"
        log = run("invalid-endpoint", ["--standalone", "--webxr-socket", str(missing)])
        assert "WebXR bridge unavailable" in log, log
        assert not missing.exists()

        sentinel = root / "passive-endpoint"
        sentinel.write_text("owned by another session")
        log = run(
            "passive-overlay",
            [
                "--music-floor-only",
                "--floor-size",
                "2x2",
                "--webxr-socket",
                str(sentinel),
            ],
        )
        assert "WebXR bridge" not in log and "WebXR session bridge" not in log, log
        assert sentinel.read_text() == "owned by another session"
        assert not (runtime / "dome-webxr-1.sock").exists()
    print(
        "PASS: occupied/invalid browser endpoints preserve Dome XR; passive overlay skips bridge"
    )


if __name__ == "__main__":
    main()
