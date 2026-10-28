"""CLI parsing and error reporting, with an injectable application for tests."""
from __future__ import annotations

import argparse
from collections.abc import Sequence
from pathlib import Path
import subprocess
import sys
import tomllib

import desktop_process

from .app import Desktop
from .commands import Command, CommandName
from .context import Context


DESCRIPTION = "Portable desktop coordination. Configuration is read-only; choices are XDG state."


def main(argv: Sequence[str] | None = None, *, app: Desktop | None = None) -> int:
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    parser.add_argument("command", help=", ".join(name.value for name in CommandName))
    parser.add_argument("args", nargs=argparse.REMAINDER)
    parsed = parser.parse_args(argv)
    try:
        command = Command.parse(parsed.command, parsed.args)
        if app is None:
            entrypoint = Path(__file__).resolve().parent.parent / "desktopctl.py"
            app = Desktop(Context.from_environment(entrypoint))
        with desktop_process.cancellation_signals():
            return app.dispatch(command)
    except desktop_process.Cancelled as error:
        return 128 + error.signum
    except KeyboardInterrupt:
        return 130
    except (ValueError, OSError, subprocess.CalledProcessError, subprocess.TimeoutExpired, tomllib.TOMLDecodeError) as error:
        print(f"desktopctl: {error}", file=sys.stderr)
        return 1
