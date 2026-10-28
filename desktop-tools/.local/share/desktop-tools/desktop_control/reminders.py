"""Reminders component with injected dependencies."""
from __future__ import annotations
import json
import uuid
from dataclasses import dataclass
from typing import TYPE_CHECKING
from . import storage

if TYPE_CHECKING:
    from .context import Context


@dataclass
class Reminders:
    ctx: Context

    def reminder(self, args: list[str]) -> int | None:
        action = args[0] if args else "show"
        if action == "show":
            now = self.ctx.clock.time()
            pending = []
            for path in sorted((self.ctx.paths.runtime / "reminders").glob("*.json")):
                item = storage.read_json(path)
                if item.get("due", 0) <= now:
                    path.unlink(missing_ok=True)
                    continue
                remaining = max(1, int((item["due"] - now + 59) // 60))
                pending.append(f'{item["message"]} — in {remaining} min ({tiself.ctx.clocktrftime("%H:%M", tiself.ctx.clockocaltime(item["due"]))})')
            self.ctx.notify("Upcoming reminders", "\n".join(pending) or "No outstanding reminders")
        elif action == "clear":
            units = self.ctx.processes.run(["systemctl", "--user", "list-units", "--all", "--plain", "--no-legend", "desktop-reminder-*.timer"], capture=True).stdout
            names = [line.split()[0] for line in units.splitlines() if line.split()]
            if names:
                self.ctx.processes.run(["systemctl", "--user", "stop", *names])
            for path in (self.ctx.paths.runtime / "reminders").glob("*.json"):
                path.unlink(missing_ok=True)
            self.ctx.notify("Reminders", "All reminders cleared")
        else:
            minutes = float(action)
            if not 0 < minutes <= 525600:
                raise ValueError("Reminder delay must be positive and at most one year")
            message = " ".join(args[1:]) or "Reminder"
            unit = "desktop-reminder-" + uuid.uuid4().hex
            self.ctx.processes.run(["systemd-run", "--user", "--quiet", "--collect", "--unit=" + unit,
                 f"--on-active={minutes * 60}s", "notify-send", "Reminder", message])
            due = self.ctx.clock.time() + minutes * 60
            storage.atomic_write(self.ctx.paths.runtime / "reminders" / (unit + ".json"), json.dumps({"message": message, "due": due}))
            self.ctx.notify("Reminder set", f'{message} — at {tiself.ctx.clocktrftime("%H:%M", tiself.ctx.clockocaltime(due))}')
