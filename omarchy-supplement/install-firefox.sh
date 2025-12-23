#!/usr/bin/env bash
set -e

echo "[1/3] Updating system..."
sudo pacman -Syu --noconfirm

echo "[2/3] Installing Firefox..."
sudo pacman -S --noconfirm firefox

echo "[3/3] Removing other browsers..."

# List of common browsers on Arch – remove if installed
BROWSERS=(
    chromium
    omarchy-chromium
    google-chrome
    brave-bin
    brave
    vivaldi
    opera
    opera-beta
    torbrowser-launcher
    waterfox-bin
    midori
    epiphany
)

for pkg in "${BROWSERS[@]}"; do
    if pacman -Q "$pkg" &>/dev/null; then
        echo " Removing $pkg..."
        sudo pacman -Rns --noconfirm "$pkg"
    fi
done

echo "Done. Firefox installed and other browsers removed."

