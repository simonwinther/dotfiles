"""Preferences component with injected dependencies."""
from __future__ import annotations
import json
import re
import tomllib
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context
    from .storage import StateStore


@dataclass
class Preferences:
    ctx: Context
    store: StateStore

    def seed_preferences(self) -> None:
        for kind, destination in [("config", self.ctx.paths.config), ("state", self.ctx.paths.state), ("home", self.ctx.paths.home)]:
            defaults = self.ctx.paths.data / "desktop-defaults" / kind
            if not defaults.is_dir():
                continue
            for source in defaults.rglob("*"):
                if not source.is_file():
                    continue
                target = destination / source.relative_to(defaults)
                if target.exists() or target.is_symlink():
                    continue
                # State toggles can be deliberately removed; seed them only once.
                if kind == "state" and (self.ctx.paths.state / "initial-preferences-seeded").exists():
                    continue
                storage.atomic_write(target, source.read_bytes())
        storage.atomic_write(self.ctx.paths.state / "initial-preferences-seeded", "")


    def seed_application_preferences(self) -> None:
        marker = self.ctx.paths.state / "application-preferences-seeded"
        if marker.exists() or not all(self.ctx.processes.which(name) for name in ("dconf", "gsettings")):
            return
        complete = True
        for schema, values in storage.read_json(self.ctx.paths.config / "desktop/applications.json").items():
            for key, value in values.items():
                path = "/" + schema.replace(".", "/") + "/" + key
                existing = self.ctx.processes.run(["dconf", "read", path], check=False, capture=True)
                if existing.returncode:
                    complete = False
                    continue
                if existing.stdout.strip():
                    continue
                result = self.ctx.processes.run(["gsettings", "set", schema, key, json.dumps(value)], check=False)
                complete = complete and not result.returncode
        if complete:
            storage.atomic_write(marker, "")


    def prepare_break_reminders(self) -> None:
        defaults = self.ctx.paths.config / "breakd/default.toml"
        preferences = self.ctx.paths.config / "breakd/config.toml"
        if defaults.is_file() and not preferences.exists():
            storage.atomic_write(preferences, defaults.read_text())
        if not preferences.is_file() or preferences.is_symlink():
            return
        source = preferences.read_text()
        coop = tomllib.loads(source).get("coop", {})
        if coop.get("mode") not in ("host", "guest") or coop.get("room_token"):
            return
        # A private invite must be configured before enabling authenticated co-op.
        section = re.compile(r"(?ms)^(\[coop\][^\n]*\n)(.*?)(?=^\[|\Z)")
        def disable_unconfigured_coop(match):
            content = re.sub(r'(?m)^(\s*mode\s*=\s*)[\"\'](?:host|guest)[\"\']',
                             r'\1"off"', match[2], count=1)
            return match[1] + content
        storage.atomic_write(preferences, section.sub(disable_unconfigured_coop, source, count=1))


    def integrate(self) -> None:
        self.seed_preferences()
        self.seed_application_preferences()
        self.prepare_break_reminders()
        for template, executable in (
            ("native-host.json", "chatgpt-dictate-audio-duck-host"),
            ("project-picker-native-host.json", "project-picker-host"),
        ):
            manifest = storage.read_json(self.ctx.paths.config / "desktop" / template)
            if not manifest:
                continue
            local = self.ctx.paths.home / ".local/bin" / executable
            manifest["path"] = str(local) if local.is_file() else self.ctx.processes.which(executable) or str(local)
            for browser in ("chromium", "google-chrome"):
                target = self.ctx.paths.config / browser / "NativeMessagingHosts" / (manifest["name"] + ".json")
                if not target.is_symlink():
                    storage.atomic_write(target, json.dumps(manifest, indent=2) + "\n")
