#!/bin/bash
# Status line mirroring the oh-my-posh prompt: path, git branch + status.

input=$(cat)
cwd=$(echo "$input" | jq -r '.workspace.current_dir // .cwd')
model=$(echo "$input" | jq -r '.model.display_name')
used=$(echo "$input" | jq -r '.context_window.used_percentage // empty')
weekly=$(echo "$input" | jq -r '.rate_limits.seven_day.used_percentage // empty')

# Path: ~ for home, keep the last 3 components
path="${cwd/#$HOME/~}"
IFS='/' read -ra parts <<< "$path"
n=${#parts[@]}
if [ "$n" -gt 3 ]; then
  path="…/${parts[n-3]}/${parts[n-2]}/${parts[n-1]}"
fi

# Catppuccin palette: Latte when the desktop theme is light, Mocha otherwise
if [ -e "$HOME/.local/state/desktop/theme/light.mode" ]; then
  text='38;2;76;79;105'
  subtext='38;2;92;95;119'
  overlay='38;2;156;160;176'
  green='38;2;64;160;43'
  peach='38;2;254;100;11'
else
  text='38;2;205;214;244'
  subtext='38;2;186;194;222'
  overlay='38;2;69;71;90'
  green='38;2;166;227;161'
  peach='38;2;250;179;135'
fi

out=$(printf '\033[1;%sm%s\033[0m' "$text" "$path")

# Git info (skip optional locks)
if branch=$(git -C "$cwd" --no-optional-locks symbolic-ref --short HEAD 2>/dev/null || git -C "$cwd" --no-optional-locks rev-parse --short HEAD 2>/dev/null); then
  staged=$(git -C "$cwd" --no-optional-locks diff --cached --numstat 2>/dev/null | wc -l)
  changed=$(git -C "$cwd" --no-optional-locks diff --numstat 2>/dev/null | wc -l)
  out+=$(printf ' \033[%sm│\033[0m \033[%sm %s\033[0m' "$overlay" "$subtext" "$branch")
  [ "$staged" -gt 0 ] && out+=$(printf ' \033[%sm+%s\033[0m' "$green" "$staged")
  [ "$changed" -gt 0 ] && out+=$(printf ' \033[%sm~%s\033[0m' "$peach" "$changed")
fi

out+=$(printf ' \033[%sm│\033[0m \033[%sm%s\033[0m' "$overlay" "$subtext" "$model")
[ -n "$used" ] && out+=$(printf ' \033[%sm│\033[0m \033[%smctx %.0f%%\033[0m' "$overlay" "$subtext" "$used")
[ -n "$weekly" ] && out+=$(printf ' \033[%sm│\033[0m \033[%smWKLY LIMIT %.0f%%\033[0m' "$overlay" "$subtext" "$weekly")

printf '%s' "$out"
