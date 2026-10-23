# OBS settings

Reviewed desktop scenes, profiles, audio filters and assets are in
`.local/share/obs-defaults`. The helper `obs-config-sync` copies them to writable
runtime files and preserves private credentials. Existing OBS files are backed
up under XDG state before applying changed defaults. Close OBS first; use
`--force` to reapply unchanged defaults deliberately.

NixOS uses Home Manager. Other distributions may Stow this package and install
OBS, Python 3 and the URW Bookman font. See `docs/OBS.md` in the Nix configuration
for source provenance, the exact filter chain and runtime verification.

The existing `countdown.lua` script is the OBS Project's countdown example by
Lain, retained with its attribution. OBS Studio and its example scripts are
distributed under GPL-2.0-or-later; see https://github.com/obsproject/obs-studio.
