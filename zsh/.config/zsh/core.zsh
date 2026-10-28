[[ -o interactive ]] || return

# Export
export PATH="$HOME/sdk/flutter/bin:$PATH"
export PATH=$HOME/.opencode/bin:$PATH
export CHROME_EXECUTABLE=chromium

# ──────────────────────────── GHCUP ────────────────────────────
[ -f "$HOME/.ghcup/env" ] && . "$HOME/.ghcup/env"

# ──────────────────────────── Zoxide ────────────────────────────
source ~/.config/zsh/tool-init.zsh
__dotfiles_cached_init zoxide '' init zsh

# ──────────────────────────── OH MY POSH ────────────────────────────
eval "$(oh-my-posh init zsh --config ~/.config/oh-my-posh/custom-respects-waybar.omp.json)"
unfunction __dotfiles_cached_init
#eval "$(oh-my-posh init zsh --config https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/refs/heads/main/themes/tokyo.omp.json)"

# ──────────────────────────── HISTORY ────────────────────────────
HISTFILE=$HOME/.zhistory 
SAVEHIST=10000
HISTSIZE=10000
setopt share_history
setopt hist_expire_dups_first
setopt hist_ignore_dups 
setopt hist_verify
setopt hist_ignore_space
setopt hist_reduce_blanks

if (( $+commands[atuin] )); then
  eval "$(atuin init zsh --disable-up-arrow --disable-ctrl-r --disable-ai)"
fi

# ──────────────────────────── TEXINPUTS ────────────────────────────
export TEXINPUTS=$HOME/Desktop/acl-style-files-master//:

# Load optional version managers only when first used, and only if installed.
source ~/.config/zsh/lazy-tools.zsh

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
if (( ! $+functions[compdef] )); then
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
fi
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
source "${ZSH_AUTOSUGGESTIONS_FILE:-/usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh}" 2>/dev/null
source "${ZSH_SYNTAX_HIGHLIGHTING_FILE:-/usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh}" 2>/dev/null
