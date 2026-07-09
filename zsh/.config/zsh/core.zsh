[[ -o interactive ]] || return

# Export
export PATH="$HOME/sdk/flutter/bin:$PATH"
export PATH=/home/simon/.opencode/bin:$PATH
export CHROME_EXECUTABLE=/usr/bin/chromium

# ──────────────────────────── GHCUP ────────────────────────────
[ -f "$HOME/.ghcup/env" ] && . "$HOME/.ghcup/env"

# ──────────────────────────── Zoxide ────────────────────────────
eval "$(zoxide init zsh)"

# ──────────────────────────── OH MY POSH ────────────────────────────
eval "$(oh-my-posh init zsh --config ~/.config/oh-my-posh/custom-respects-waybar.omp.json)"
#eval "$(oh-my-posh init zsh --config https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/refs/heads/main/themes/tokyo.omp.json)"

# ──────────────────────────── HISTORY ────────────────────────────
HISTFILE=$HOME/.zhistory 
SAVEHIST=10000
HISTSIZE=10000
setopt share_history
setopt hist_expire_dups_first
setopt hist_ignore_dups 
setopt hist_verify
setopt inc_append_history
setopt hist_ignore_space
setopt hist_reduce_blanks

# ──────────────────────────── TEXINPUTS ────────────────────────────
export TEXINPUTS=$HOME/Desktop/acl-style-files-master//:

# ──────────────────────────── NVM ────────────────────────────
# Lazy loading NVM
export NVM_DIR="$HOME/.nvm"

nvm() {
  unset -f nvm node npm npx
  [[ -s "$NVM_DIR/nvm.sh" ]] && source "$NVM_DIR/nvm.sh"
  command nvm "$@"
}

node() { nvm >/dev/null; command node "$@"; }
npm()  { nvm >/dev/null; command npm  "$@"; }
npx()  { nvm >/dev/null; command npx  "$@"; }

# ──────────────────────────── SDKMAN ────────────────────────────
# Lazy loading SDKMAN
export SDKMAN_DIR="$HOME/.sdkman"

sdk() {
  unset -f sdk
  [[ -s "$SDKMAN_DIR/bin/sdkman-init.sh" ]] && source "$SDKMAN_DIR/bin/sdkman-init.sh"
  command sdk "$@"
}

# ──────────────────────────── ZLE VI MODE ────────────────────────────
bindkey -v
KEYTIMEOUT=1   # 10ms: snappy Esc; multi-key vi seqs still work (burst input)

# ───────── My overrides ─────────
source ~/.config/zsh/functions/init.zsh

# keybind widgets must be sourced 
for f in ~/.config/zsh/keybinds/*.zsh; do
  [[ "$f" == *"/init.zsh" ]] && continue
  source "$f"
done

source ~/.config/zsh/keybinds/init.zsh
source ~/.config/zsh/aliases.zsh

# Load local secrets if present
[[ -f ~/.config/zsh/secrets.zsh ]] && source ~/.config/zsh/secrets.zsh

# ──────────────────────────── COMPLETION ────────────────────────────
autoload -Uz compinit
() {
  emulate -L zsh
  local dump=${ZDOTDIR:-$HOME}/.zcompdump
  local -a old=( $dump(N.mh+24) )    # non-empty if the dump exists and is >24h old
  if [[ -f $dump ]] && (( ! $#old )); then
    compinit -C -d $dump             # fresh (<24h): skip the security scan, fast
  else
    compinit -d $dump                # missing or stale: full init + regenerate
  fi
}
zstyle ':completion:*' menu select                          # arrow-key menu
zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}'   # case-insensitive

# Shift-Tab cycles completions backwards (Tab goes forwards) — both when
# starting completion and while moving through the highlighted menu.
zmodload -i zsh/complist
bindkey '^[[Z' reverse-menu-complete
bindkey -M menuselect '^[[Z' reverse-menu-complete

# ──────────────────────────── ZSH PLUGINS ────────────────────────────
# Source order matters: vi-mode defines its widgets first, then
# zsh-autosuggestions wraps them (MANUAL_REBIND binds once instead of on every
# prompt), and zsh-syntax-highlighting must come last so it wraps everything.
ZSH_AUTOSUGGEST_USE_ASYNC=1
ZSH_AUTOSUGGEST_MANUAL_REBIND=1

source ~/.config/zsh/vi-mode.zsh
source /usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh 2>/dev/null
source /usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh 2>/dev/null
