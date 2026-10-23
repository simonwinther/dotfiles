#!/usr/bin/env python3
"""Browse projects from a local, browser-maintained snapshot."""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import time
import unicodedata

from desktopctl import atomic_write

MAIN_PAGE = "https://chatgpt.com"
EXTENSION_ORIGIN = "chrome-extension://lfiijailpfkonkfmfkcjfagnopohffge/"
MAX_MESSAGE = 4 * 1024 * 1024
STATUS_MESSAGES = {
    "syncing": "Syncing projects…",
    "auth_required": "Sync failed · Sign in to the website",
    "workspace_unknown": "Sync failed · Could not identify the selected workspace",
    "invalid_response": "Sync failed · The website returned an unsupported project list",
    "pagination": "Sync failed · Project pagination did not finish",
    "account_changed": "Sync interrupted · Account or workspace changed",
    "network": "Sync failed · Browser network request failed",
    "timeout": "Sync failed · Browser request timed out",
    "unexpected": "Sync failed · Check the extension's status in Chromium",
    "ok": "Projects synced",
}


def save_status(value, path=None):
    code = value.get("code")
    if not isinstance(code, str) or (code not in STATUS_MESSAGES and not re.fullmatch(r"http_[1-5][0-9]{2}", code)):
        raise ValueError("Unknown sync status")
    path = path or cache_path().with_name("status.json")
    result = {"code": code, "at": int(time.time() * 1000)}
    if value.get("stage") in {"session", "workspace", "projects"}:
        result["stage"] = value["stage"]
    if value.get("kind") in {"TypeError", "SyntaxError", "TimeoutError", "Error"}:
        result["kind"] = value["kind"]
    atomic_write(path, json.dumps(result) + "\n")
    return {"ok": True}


def sync_status(path, since=0):
    try:
        value = json.loads(path.read_text())
        code = value["code"]
        if value["at"] <= since or code == "ok":
            return ""
        if code == "syncing" and time.time() * 1000 - value["at"] > 120000:
            return "Sync interrupted · Refresh the website to retry"
        if re.fullmatch(r"http_[1-5][0-9]{2}", code):
            return f"Sync failed · Website request returned HTTP {code[5:]}"
        details = [value[key] for key in ("stage", "kind") if key in value]
        return STATUS_MESSAGES[code] + (" · " + " / ".join(details) if details else "")
    except (OSError, ValueError, KeyError, TypeError):
        return ""


def cache_path():
    return Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "project-picker/projects.json"


def label(value):
    return " ".join("".join(" " if unicodedata.category(c).startswith("C") else c for c in value).split())


def text(value, field):
    if not isinstance(value, str) or not value.strip() or len(value) > 1024:
        raise ValueError(f"Invalid {field}")
    return value


def validate_snapshot(value):
    if not isinstance(value, dict) or value.get("version") != 1 or value.get("complete") is not True:
        raise ValueError("Expected a complete version 1 snapshot")
    result = {"version": 1, "complete": True}
    for field in ("account_id", "workspace_id", "workspace_name"):
        result[field] = text(value.get(field), field)
    started = value.get("started_at")
    if type(started) is not int or not 0 < started <= int(time.time() * 1000) + 60000:
        raise ValueError("Invalid snapshot time")
    result["started_at"] = started
    projects = value.get("projects")
    if not isinstance(projects, list) or len(projects) > 10000:
        raise ValueError("Invalid project list")
    result["projects"] = []
    seen = set()
    for project in projects:
        if not isinstance(project, dict):
            raise ValueError("Invalid project")
        project_id = project.get("id")
        if not isinstance(project_id, str) or not re.fullmatch(r"g-p-[0-9a-f]{32}", project_id):
            raise ValueError("Invalid project ID")
        if project_id in seen:
            raise ValueError("Duplicate project ID")
        seen.add(project_id)
        name = text(project.get("name"), "project name")
        if not label(name):
            raise ValueError("Empty project label")
        url = f"{MAIN_PAGE}/g/{project_id}/project"
        if project.get("url") != url:
            raise ValueError("Invalid project URL")
        result["projects"].append({"id": project_id, "name": name, "url": url})
    result["projects"].sort(key=lambda project: (label(project["name"]).casefold(), project["id"]))
    return result


def read_cache(path):
    try:
        value = json.loads(path.read_text())
    except FileNotFoundError:
        return {"version": 1, "active": None, "contexts": {}}
    if (not isinstance(value, dict) or value.get("version") != 1 or not isinstance(value.get("contexts"), dict)
            or not (value.get("active") is None or isinstance(value.get("active"), str))
            or any(not isinstance(context, dict) or type(context.get("started_at")) is not int
                   for context in value["contexts"].values())):
        raise ValueError("Unsupported project cache")
    return value


def save_snapshot(value, path=None):
    snapshot = validate_snapshot(value)
    path = path or cache_path()
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    key = hashlib.sha256(json.dumps([snapshot["account_id"], snapshot["workspace_id"]]).encode()).hexdigest()
    with (path.parent / "sync.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        cache = read_cache(path)
        previous = cache["contexts"].get(key)
        if previous and previous["started_at"] >= snapshot["started_at"]:
            return {"ok": True, "updated": False}
        snapshot["synced_at"] = int(time.time() * 1000)
        cache["contexts"][key] = snapshot
        active = cache["contexts"].get(cache.get("active"))
        if not active or active["started_at"] <= snapshot["started_at"]:
            cache["active"] = key
        atomic_write(path, json.dumps(cache, ensure_ascii=False) + "\n")
        save_status({"code": "ok"}, path.with_name("status.json"))
    return {"ok": True, "updated": True, "count": len(snapshot["projects"])}


def read_exact(stream, length):
    result = bytearray()
    while len(result) < length:
        chunk = stream.read(length - len(result))
        if not chunk:
            raise ValueError("Incomplete native message")
        result.extend(chunk)
    return bytes(result)


def native_host(origin, source=None, destination=None):
    source = source if source is not None else sys.stdin.buffer
    destination = destination if destination is not None else sys.stdout.buffer
    try:
        if origin != EXTENSION_ORIGIN:
            raise ValueError("Unexpected extension origin")
        size = struct.unpack("=I", read_exact(source, 4))[0]
        if not 0 < size <= MAX_MESSAGE:
            raise ValueError("Native message exceeds the size limit")
        value = json.loads(read_exact(source, size))
        if isinstance(value, dict) and value.get("action") == "status":
            result = save_status(value)
        else:
            result = save_snapshot(value)
    except (ValueError, OSError, UnicodeError, KeyError, TypeError):
        result = {"ok": False, "error": "Could not save a complete project snapshot"}
    data = json.dumps(result).encode()
    destination.write(struct.pack("=I", len(data)) + data)
    destination.flush()
    return 0 if result["ok"] else 1


def entries(path=None):
    home = {"id": "home", "name": "Open main page", "url": MAIN_PAGE}
    path = path or cache_path()
    try:
        cache = read_cache(path)
        snapshot = cache["contexts"].get(cache.get("active"))
        if snapshot:
            validated = validate_snapshot(snapshot)
            synced = time.strftime("%Y-%m-%d %H:%M", time.localtime(snapshot["synced_at"] / 1000))
            header = f"{label(validated['workspace_name'])} · Last sync: {synced}"
            error = sync_status(path.with_name("status.json"), snapshot["synced_at"])
            if error:
                header += "\n" + error
            return [home, *validated["projects"]], header
    except (ValueError, KeyError, TypeError, OverflowError, OSError):
        return [home], "Saved list unreadable · Check the picker cache"
    return [home], sync_status(path.with_name("status.json")) or "No sync yet · Open main page with Project Picker Sync enabled"


def open_project(url):
    subprocess.Popen(["desktopctl", "launch", "webapp", url], start_new_session=True,
                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def choose():
    binary = shutil.which("project-picker-tui")
    if not binary:
        candidate = cache_path().parent / "bin/project-picker-tui"
        if candidate.is_file() and os.access(candidate, os.X_OK):
            binary = str(candidate)
    if not binary:
        raise ValueError("Rust picker unavailable; install the native package or run project-picker-build")
    result = subprocess.run([binary, "--cache", str(cache_path())],
                            text=True, stdout=subprocess.PIPE)
    if result.returncode == 130:
        return 0
    if result.returncode:
        raise ValueError("Project search failed")
    selected = result.stdout.strip()
    if not selected:
        return 0
    projects, _ = entries()
    project = next((project for project in projects if project["id"] == selected), None)
    if project is None:
        raise ValueError("This project is no longer available; reopen the picker")
    open_project(project["url"])
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--choose", action="store_true", help="Run the picker inside the current terminal")
    parser.add_argument("--list", action="store_true", help="Print cached projects without opening a window")
    parser.add_argument("--native-host", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("origin", nargs="?", help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if args.native_host:
        return native_host(args.origin)
    try:
        if args.list:
            projects, _ = entries()
            for project in projects:
                print(f"{label(project['name'])}\t{project['url']}")
            return 0
        if args.choose:
            return choose()
        os.execvp("desktopctl", ["desktopctl", "terminal", "--float", "--app-id", "project.picker", "project-picker", "--choose"])
    except (ValueError, OSError) as error:
        print(f"project-picker: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
