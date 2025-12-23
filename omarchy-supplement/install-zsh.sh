#!/usr/bin/env bash
set -e

# Install Zsh
if ! command -v zsh &>/dev/null; then
  yay -S --noconfirm --needed zsh
fi

yay -S oh-my-posh zsh-autosuggestions zsh-syntax-highlighting
