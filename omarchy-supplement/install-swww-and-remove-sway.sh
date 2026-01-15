#!/usr/bin/env bash
set -e

#Install swww
yay -S --noconfirm swww

# Remove sway from omarchy
yay -Rns --noconfirm sway

# Also, grep for swaybg in HOME dir, so u can switch everything omarchy uses with swaybg.
# OR just switch to nix, to relieve yourself from this eternal pain youre going through with omarchy
