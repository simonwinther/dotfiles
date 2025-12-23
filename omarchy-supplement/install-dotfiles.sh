#!/usr/bin/env bash

set -euo pipefail

ORIGINAL_DIR=$(pwd)
REPO_URL="https://github.com/simonwinther/dotfiles"
REPO_NAME="dotfiles"
REPO_DIR="$HOME/$REPO_NAME"

STOW_PACKAGES=(
  ghostty
  lazygit
  nvim
  oh-my-posh
  tmux
  waybar
  zsh
)

CONFIGS_TO_REMOVE=(
  "$HOME/.config/nvim"
  "$HOME/.local/share/nvim"
  "$HOME/.cache/nvim"
  "$HOME/.config/ghostty/config"
  "$HOME/.config/lazygit/config.yml"
)

is_stow_installed() {
  command -v stow &>/dev/null
}

clone() {
  if [ -d "$REPO_DIR/.git" ]; then
    echo "Repository '$REPO_NAME' already exists."
    echo "Delete ~/dotfiles and try again."
  else
    echo "Cloning repository '$REPO_URL' into '$REPO_DIR'..."
    git clone "$REPO_URL" "$REPO_DIR"
  fi
}

remove_old_configs() {
  echo "Removing old configs..."
  for path in "${CONFIGS_TO_REMOVE[@]}"; do
    if [ -e "$path" ]; then
      echo "  Removing $path"
      rm -rf -- "$path"
    fi
  done
}

run_stow() {
  cd "$REPO_DIR"
  echo "Stowing packages..."
  for pkg in "${STOW_PACKAGES[@]}"; do
    echo "  stow $pkg"
    stow "$pkg"
  done
}

main() {
  if ! is_stow_installed; then
    echo "Error: 'stow' is not installed."
    echo "On Arch, install it with: sudo pacman -S stow"
    exit 1
  fi

  clone
  remove_old_configs
  run_stow

  cd "$ORIGINAL_DIR"
  echo "Done ✅"
}

main "$@"
