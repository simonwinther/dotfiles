#!/usr/bin/env python3
"""Use the existing KU token privately, independent of browser profiles."""
import json
import os
from pathlib import Path
import stat
from firefox_totp import generate, validate_entry

CONFIG = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'hendrix/totp.json'


def read_entry(config_path=CONFIG):
    path = Path(config_path)
    metadata = path.stat()
    if metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) & 0o077:
        raise PermissionError('The saved KU authenticator must be private to your user (0600).')
    entry = validate_entry(json.loads(path.read_text()))
    generate(entry['secret'], period=int(entry.get('period') or 30), digits=int(entry.get('digits') or 6))
    return entry
