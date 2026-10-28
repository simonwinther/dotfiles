"""Desktop preference schema, validation, and version migrations."""
from __future__ import annotations
import json
import math
from pathlib import Path
import re


STATE_VERSION = 1


def state_text(value):
    return isinstance(value, str) and bool(value.strip()) and not re.search(r"[\x00-\x1f\x7f-\x9f\ud800-\udfff]", value)


STATE_SCHEMA = {
    "theme": ("a lowercase theme name", lambda v: state_text(v) and re.fullmatch(r"[a-z0-9][a-z0-9_-]*", v)),
    "font": ("a nonempty font family without control characters", state_text),
    "wallpaper": ("a nonempty image path without control characters", state_text),
    "lockBackground": ("a nonempty image path without control characters", state_text),
    "terminalTheme": ("a boolean", lambda v: type(v) is bool),
    "desktopTheme": ("a boolean", lambda v: type(v) is bool),
    "aboutArt": ("a boolean", lambda v: type(v) is bool),
    "screensaver": ("naruto, tomoe-sharingan, matrix, or custom", lambda v: isinstance(v, str) and v in ("naruto", "tomoe-sharingan", "matrix", "custom")),
}


STATE_MIGRATIONS = {0: lambda value: {**value, "schemaVersion": 1}}


class UnsupportedStateVersion(ValueError):
    pass


def check_json_depth(value):
    pending = [(value, 0)]
    while pending:
        item, depth = pending.pop()
        if depth > 64:
            raise ValueError("JSON nesting exceeds the maximum depth of 64")
        children = item.values() if isinstance(item, dict) else item if isinstance(item, list) else ()
        pending.extend((child, depth + 1) for child in children if isinstance(child, (dict, list)))


def decode_json(raw: bytes, path: Path):
    def unique_object(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise ValueError("duplicate object key")
            value[key] = item
        return value

    def reject_constant(_):
        raise ValueError("non-finite numbers are not valid JSON")

    def finite_float(text):
        value = float(text)
        if not math.isfinite(value):
            reject_constant(text)
        return value

    try:
        value = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object,
                           parse_constant=reject_constant, parse_float=finite_float)
        check_json_depth(value)
        return value
    except UnicodeDecodeError as error:
        raise ValueError(f"{path}: invalid UTF-8 at byte {error.start}") from None
    except json.JSONDecodeError as error:
        raise ValueError(f"{path}: invalid JSON at line {error.lineno}, column {error.colno}: {error.msg}") from None
    except RecursionError:
        raise ValueError(f"{path}: JSON nesting is too deep") from None
    except ValueError as error:
        raise ValueError(f"{path}: invalid JSON: {error}") from None


def normalize_state(value):
    """Return migrated state and diagnostics, dropping only invalid choices."""
    issues = []
    if not isinstance(value, dict):
        value = {}
        issues.append("$: expected a JSON object")
    value = dict(value)
    version = value.get("schemaVersion", 0)
    if type(version) is int and version > STATE_VERSION:
        raise UnsupportedStateVersion(
            f"$.schemaVersion: version {version} is newer than supported version {STATE_VERSION}; "
            "use an updated desktopctl or restore a compatible state file; state was left unchanged")
    if type(version) is not int or version < 0:
        issues.append("$.schemaVersion: expected a nonnegative integer")
        value.pop("schemaVersion", None)
        version = 0
    for key, (description, valid) in STATE_SCHEMA.items():
        if key in value and not valid(value[key]):
            issues.append(f"$.{key}: expected {description}")
            del value[key]
    if "wallpapers" in value:
        wallpapers = value["wallpapers"]
        if not isinstance(wallpapers, dict):
            issues.append("$.wallpapers: expected an object mapping outputs to image paths")
            del value["wallpapers"]
        else:
            value["wallpapers"] = {}
            for output, image in wallpapers.items():
                if not state_text(output) or not state_text(image):
                    # Do not log paths or untrusted output names from private state.
                    issues.append("$.wallpapers: each output and image path must be nonempty text without control characters")
                else:
                    value["wallpapers"][output] = image
    while version < STATE_VERSION:
        value = STATE_MIGRATIONS[version](value)
        version = value["schemaVersion"]
    return value, issues
