#!/usr/bin/env python3
"""Materialize reviewed OBS defaults as writable files, retaining runtime secrets."""

import argparse
import configparser
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import tempfile
import time


def ini(text):
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str
    parser.read_string(text)
    return parser


def merge_json(current, declared):
    result = dict(current) if isinstance(current, dict) else {}
    for key, value in declared.items():
        result[key] = merge_json(result.get(key), value) if isinstance(value, dict) else value
    return result


def render(source, destination, substitutions):
    text = source.read_text()
    if source.suffix == ".json":
        # Substitute decoded strings so quotes/backslashes in home paths stay valid.
        def expand(value):
            if isinstance(value, str):
                for before, after in substitutions.items():
                    value = value.replace(before, after)
            elif isinstance(value, dict):
                value = {k: expand(v) for k, v in value.items()}
            elif isinstance(value, list):
                value = [expand(v) for v in value]
            return value

        declared = expand(json.loads(text))
        current = json.loads(destination.read_text()) if destination.exists() else {}
        if source.parent.name == "scenes":
            # Portal grants belong to this machine and must never enter the defaults.
            grants = {s.get("uuid"): s.get("settings", {}).get("RestoreToken")
                      for s in current.get("sources", [])}
            for s in declared.get("sources", []):
                token = grants.get(s.get("uuid"))
                if token and s.get("id") == "pipewire-screen-capture-source":
                    s.setdefault("settings", {})["RestoreToken"] = token
        else:
            # service.json / websocket: public settings override, secret keys survive.
            declared = merge_json(current, declared)
        return (json.dumps(declared, indent=2, ensure_ascii=False) + "\n").encode()
    for before, after in substitutions.items():
        text = text.replace(before, after)
    if source.suffix == ".ini":
        declared = ini(text)
        current = ini(destination.read_text()) if destination.exists() else ini("")
        for section in declared.sections():
            if section not in current:
                current.add_section(section)
            current[section].update(declared[section])
        output = io.StringIO()
        current.write(output, space_around_delimiters=False)
        text = output.getvalue()
    return text.encode()


def obs_running():
    for process in Path("/proc").iterdir():
        if not process.name.isdecimal():
            continue
        try:
            if process.stat().st_uid == os.getuid() and (process / "comm").read_text().strip() in {
                "obs", ".obs-wrapped", "obs-studio"
            }:
                return True
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            pass
    return False


def atomic_write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        pending = Path(temporary.name)
        try:
            temporary.write(data)
            temporary.flush()
            os.fsync(temporary.fileno())
            os.replace(pending, path)
        finally:
            pending.unlink(missing_ok=True)


def synchronize(defaults, home, config_home, data_home, state_home, force=False, defer=False):
    target = config_home / "obs-studio"
    state = state_home / "obs-sync"
    substitutions = {"@HOME@": str(home), "@CONFIG_HOME@": str(config_home),
                     "@ASSETS@": str(data_home / "obs-defaults/assets")}
    files = sorted(p for p in defaults.rglob("*") if p.is_file())
    sources = sorted(p for p in (defaults / "config").rglob("*") if p.is_file())
    if not sources:
        raise RuntimeError("OBS defaults are missing")
    digest = hashlib.sha256(json.dumps(substitutions, sort_keys=True).encode())
    for file in files:
        digest.update(str(file.relative_to(defaults)).encode() + b"\0" + file.read_bytes())
    version = digest.hexdigest()
    state.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (state / "lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        marker = state / "applied.json"
        if not force and marker.exists() and json.loads(marker.read_text())["version"] == version:
            return "OBS defaults already synchronized; runtime edits preserved."
        if obs_running():
            if defer:
                return "OBS is running; defaults deferred. Close OBS and run obs-config-sync."
            raise RuntimeError("Close OBS before synchronizing its settings")
        plan = []
        for source in sources:
            relative = source.relative_to(defaults / "config")
            destination = target / relative
            if any(p.is_symlink() for p in [destination, *destination.parents] if p != config_home):
                raise RuntimeError("OBS runtime files must be writable files, not managed links")
            plan.append((relative, destination, render(source, destination, substitutions)))
        # Parse every input before changing anything. Backups include credentials and
        # therefore stay private, outside the checkout and Nix store.
        backup = state / "backups" / str(time.time_ns())
        for relative, destination, _ in plan:
            if destination.exists():
                saved = backup / relative
                saved.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                shutil.copyfile(destination, saved)
                saved.chmod(0o600)
        for _, destination, content in plan:
            atomic_write(destination, content)
        atomic_write(marker, (json.dumps({"version": version}) + "\n").encode())
        return f"Synchronized {len(plan)} OBS files. Previous files: {backup}"


def main():
    home = Path.home()
    config_home = Path(os.environ.get("XDG_CONFIG_HOME", home / ".config"))
    data_home = Path(os.environ.get("XDG_DATA_HOME", home / ".local/share"))
    state_home = Path(os.environ.get("XDG_STATE_HOME", home / ".local/state"))
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--defaults", type=Path, default=data_home / "obs-defaults")
    parser.add_argument("--force", action="store_true", help="Reapply defaults even if unchanged")
    parser.add_argument("--defer-if-running", action="store_true", help="Defer updates while OBS is open")
    args = parser.parse_args()
    os.umask(0o077)
    try:
        print(synchronize(args.defaults.absolute(), home, config_home, data_home, state_home,
                          args.force, args.defer_if_running))
    except RuntimeError as error:
        parser.exit(1, f"{error}\n")
    except (OSError, ValueError, configparser.Error) as error:
        # Parser diagnostics can contain credentials from a broken runtime file.
        parser.exit(1, f"OBS synchronization failed ({type(error).__name__}); no secrets logged.\n")


if __name__ == "__main__":
    main()
