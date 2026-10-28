#!/usr/bin/env python3
"""Deploy only packages in packages.json; default to GNU Stow simulation."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys


def load_manifest(root):
    manifest = json.loads((root / "packages.json").read_text())
    if (not isinstance(manifest, dict)
            or set(manifest) != {"version", "packages", "ignore"}
            or type(manifest["version"]) is not int or manifest["version"] != 1):
        raise ValueError("packages.json must have version 1, packages and ignore fields")
    packages = manifest["packages"]
    if (not isinstance(packages, list) or not packages
            or not all(isinstance(name, str) and re.fullmatch(r"[a-z0-9][a-z0-9-]*", name)
                       for name in packages)):
        raise ValueError("packages.json packages must be a nonempty list of package names")
    if packages != sorted(set(packages)):
        raise ValueError("packages.json packages must be unique and alphabetically sorted")
    for name in packages:
        package = root / name
        if package.is_symlink() or not package.is_dir():
            raise ValueError(f"Manifest package is missing or is not a real directory: {name}")
    ignore = manifest["ignore"]
    if not isinstance(ignore, dict) or set(ignore) != {"names", "suffixes", "rootNames"}:
        raise ValueError("packages.json ignore must have names, suffixes and rootNames fields")
    for key, values in ignore.items():
        if (not isinstance(values, list)
                or not all(isinstance(value, str) and value not in {"", ".", ".."}
                           and not any(c in value for c in "/\n\r\0") for value in values)):
            raise ValueError(f"packages.json ignore.{key} must contain literal path components")
    return manifest


def ignore_options(ignore):
    """Literal manifest rules become anchored Stow command-line regexes."""
    options = []
    for key, prefix in [("names", "(^|/)"), ("suffixes", ""), ("rootNames", "^")]:
        if ignore[key]:
            patterns = "|".join(re.escape(value) for value in ignore[key])
            options.append(f"--ignore={prefix}({patterns})$")
    return options


def deploy(root, target, apply=False):
    manifest = load_manifest(root)
    target = target.resolve()
    if not target.is_dir():
        raise ValueError("Target directory must already exist")
    if apply and Path("/etc/NIXOS").exists() and target == Path.home().resolve():
        raise ValueError("Home Manager owns these targets on NixOS. Use nixos-rebuild.")
    command = ["stow", f"--dir={root}", f"--target={target}", "--no-folding",
               *ignore_options(manifest["ignore"])]
    # Stow still checks its own default/local ignores and all target conflicts.
    subprocess.run([*command, "--simulate", "--verbose", "--restow", *manifest["packages"]],
                   cwd=root, check=True)
    if apply:
        subprocess.run([*command, "--restow", *manifest["packages"]], cwd=root, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--apply", action="store_true")
    mode.add_argument("--dry-run", action="store_true", help="simulate (the default)")
    args = parser.parse_args()
    try:
        deploy(Path(__file__).resolve().parent, args.target, args.apply)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"stow-portable: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
