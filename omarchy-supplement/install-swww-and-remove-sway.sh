#!/usr/bin/env bash
set -e

#Install awww
yay -S --noconfirm awww #works with waypaper-git rn, not waypaper.

# Remove sway from omarchy
yay -Rns --noconfirm sway

# Also, grep for swaybg in HOME dir, so u can switch everything omarchy uses with swaybg.
# OR just switch to nix, to relieve yourself from this eternal pain youre going through with omarchy
