#!/usr/bin/env python3
"""Read one selected Firefox Authenticator entry and generate its current code."""

import base64
import hmac
import json
from pathlib import Path
import sqlite3
import struct
import time

CONFIG = Path.home() / '.config/hendrix/firefox-authenticator.json'
EXTENSION_ID = 'authenticator@mymindstorm'


def read_entry(config_path=CONFIG):
    config = json.loads(Path(config_path).read_text())
    database = Path(config['profile']) / 'storage-sync-v2.sqlite'
    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True, timeout=3) as conn:
        conn.execute('PRAGMA trusted_schema=OFF')
        row = conn.execute('SELECT data FROM storage_sync_data WHERE ext_id=?',
                           (EXTENSION_ID,)).fetchone()
    if not row or not row[0]:
        raise ValueError('Firefox Authenticator data is unavailable.')
    entry = json.loads(row[0]).get(config['entry_id'])
    return validate_entry(entry)


def validate_entry(entry):
    if not isinstance(entry, dict) or entry.get('issuer', '').casefold() != 'netiq':
        raise ValueError('The selected Netiq authenticator is unavailable.')
    if entry.get('encrypted'):
        raise ValueError('Firefox Authenticator is password-protected; unlock or enter a code manually.')
    if entry.get('type') not in (1, 'totp'):
        raise ValueError('The selected authenticator is not a time-based token.')
    if entry.get('algorithm', 1) not in (1, 'SHA1'):
        raise ValueError('The selected authenticator uses an unsupported algorithm.')
    return entry


def generate(secret, timestamp=None, period=30, digits=6, algorithm='sha1'):
    if not isinstance(period, int) or not 1 <= period <= 3600:
        raise ValueError('Invalid authenticator period.')
    if not isinstance(digits, int) or not 6 <= digits <= 8:
        raise ValueError('Invalid authenticator code length.')
    normalized = ''.join(secret.split()).upper().rstrip('=')
    key = base64.b32decode(normalized + '=' * (-len(normalized) % 8))
    if not key:
        raise ValueError('The authenticator seed is empty.')
    now = time.time() if timestamp is None else timestamp
    digest = hmac.digest(key, struct.pack('>Q', int(now // period)), algorithm)
    offset = digest[-1] & 15
    value = struct.unpack('>I', digest[offset:offset + 4])[0] & 0x7fffffff
    return str(value % (10 ** digits)).zfill(digits)


def current_code():
    entry = read_entry()
    return generate(entry['secret'], period=int(entry.get('period') or 30),
                    digits=int(entry.get('digits') or 6))
