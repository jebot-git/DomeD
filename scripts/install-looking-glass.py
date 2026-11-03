#!/usr/bin/env python3
"""Prepare or apply a verified Looking Glass compositor/helper update, without restarting sessions."""

import argparse, datetime, fcntl, hashlib, json, os, pathlib, shutil, subprocess, tempfile

P = pathlib.Path
root = P(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument(
    "--build", type=P, help="Directory containing release dome-compositor and dome-xr"
)
parser.add_argument(
    "--apply",
    type=P,
    help="Apply an already prepared plan (native sudo authentication required)",
)
parser.add_argument("--verify-only", action="store_true")
args = parser.parse_args()
allowed = {
    f"{prefix}/{name}"
    for prefix in ["usr/local/bin", "opt/fltk-vr/bin"]
    for name in ["dome-compositor", "dome-xr"]
}
allowed |= {
    "usr/local/share/doc/domed/LOOKING-GLASS.rst",
    "usr/local/share/doc/domed/looking-glass-source.json",
}


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify(plan):
    if {item["path"] for item in plan["files"]} != allowed:
        raise SystemExit("Unexpected install file set")
    stage = P(plan["stage"])
    for item in plan["files"]:
        source = stage / item["path"]
        if (
            source.is_symlink()
            or not source.is_file()
            or sha(source) != item["sha256"]
            or item["mode"] not in [0o644, 0o755]
        ):
            raise SystemExit("Invalid staged file: " + item["path"])
    print("Verified", len(plan["files"]), "staged files", flush=True)


if not args.apply:
    if not args.build:
        parser.error("--build RELEASE_DIR is required to prepare the update")
    artifacts = root / "artifacts/looking-glass"
    artifacts.mkdir(parents=True, exist_ok=True)
    stage = P(tempfile.mkdtemp(prefix="install-", dir=artifacts))
    for name in ["dome-compositor", "dome-xr"]:
        for prefix in ["usr/local/bin", "opt/fltk-vr/bin"]:
            target = stage / prefix / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(args.build / name, target)
            target.chmod(0o755)
            subprocess.run(
                [
                    "patchelf",
                    "--force-rpath",
                    "--set-rpath",
                    "/opt/fltk-vr/bdf-runtime/lib:/opt/fltk-vr/toolkit/lib:/usr/local/lib/domed/bitmap",
                    str(target),
                ],
                check=True,
            )
            dependencies = subprocess.check_output(["ldd", str(target)], text=True)
            if "not found" in dependencies or any(
                token in dependencies.lower() for token in ["libqt", "libgtk", "webkit"]
            ):
                raise SystemExit("Unexpected runtime dependency: " + dependencies)
    commit = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
    ).strip()
    metadata = {
        "commit": commit,
        "upstream": "f797e0f1d43e7be5a06128ca7b0b0b0259fefabd",
        "experimental": True,
        "enabled_by_default": False,
        "headset_tested": False,
    }
    doc = stage / "usr/local/share/doc/domed"
    doc.mkdir(parents=True)
    shutil.copyfile(root / "docs/LOOKING-GLASS.rst", doc / "LOOKING-GLASS.rst")
    (doc / "looking-glass-source.json").write_text(
        json.dumps(metadata, indent=2) + "\n"
    )
    for path in stage.rglob("*"):
        if path.is_dir():
            path.chmod(0o755)
        elif path.name not in ["dome-compositor", "dome-xr"]:
            path.chmod(0o644)
    files = [
        {
            "path": str(path.relative_to(stage)),
            "sha256": sha(path),
            "mode": path.stat().st_mode & 0o777,
            "size": path.stat().st_size,
        }
        for path in sorted(stage.rglob("*"))
        if path.is_file()
    ]
    plan = {"stage": str(stage), "commit": commit, "files": files}
    verify(plan)
    target = artifacts / "install-plan.json"
    target.write_text(json.dumps(plan, indent=2) + "\n")
    print("Prepared", target)
    raise SystemExit(0)

plan = json.loads(args.apply.read_text())
verify(plan)
if args.verify_only:
    raise SystemExit(0)
if os.geteuid() != 0:
    raise SystemExit("Use sudo -A to apply this prepared system update")
os.umask(0o022)
lock = open("/run/lock/domed-install.lock", "w")
fcntl.flock(lock, fcntl.LOCK_EX)
backup = P("/var/backups/de-xr-looking-glass") / datetime.datetime.now().strftime(
    "%Y%m%d-%H%M%S-%f"
)
backup.mkdir(parents=True)
files = plan["files"]
stage = P(plan["stage"])
changed = []


def replace(source, target, mode):
    target.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
    fd, name = tempfile.mkstemp(prefix=".looking-glass-", dir=target.parent)
    try:
        with os.fdopen(fd, "wb") as out:
            with source.open("rb") as inp:
                shutil.copyfileobj(inp, out)
            out.flush()
            os.fsync(out.fileno())
            os.fchmod(out.fileno(), mode)
        os.replace(name, target)
    finally:
        P(name).unlink(missing_ok=True)


try:
    for item in files:
        target = P("/") / item["path"]
        if target.is_symlink() or any(parent.is_symlink() for parent in target.parents):
            raise RuntimeError("Refusing symlinked target " + str(target))
        if target.exists():
            saved = backup / item["path"]
            saved.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(target, saved)
    for item in files:
        target = P("/") / item["path"]
        changed.append(item["path"])
        replace(stage / item["path"], target, item["mode"])
    inventory = P("/opt/fltk-vr/install-manifest.json")
    if inventory.is_file():
        saved = backup / "opt/fltk-vr/install-manifest.json"
        saved.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(inventory, saved)
        entries = {item["path"]: item for item in json.loads(inventory.read_text())}
        entries.update(
            {
                item["path"]: item
                for item in files
                if item["path"].startswith(("usr/local/bin/", "opt/fltk-vr/bin/"))
            }
        )
        updated = backup / "updated-inventory.json"
        updated.write_text(json.dumps(list(entries.values()), indent=2) + "\n")
        changed.append("opt/fltk-vr/install-manifest.json")
        replace(updated, inventory, 0o644)
    for item in files:
        if sha(P("/") / item["path"]) != item["sha256"]:
            raise RuntimeError("Installed hash mismatch: " + item["path"])
    (backup / "install-plan.json").write_text(json.dumps(plan, indent=2) + "\n")
except BaseException:
    for name in reversed(changed):
        target = P("/") / name
        saved = backup / name
        if saved.exists():
            replace(saved, target, saved.stat().st_mode & 0o777)
        else:
            target.unlink(missing_ok=True)
    raise
print("Installed and verified", len(files), "files; backups:", backup)
print("Running sessions were left running. Enable the prototype after a new login.")
