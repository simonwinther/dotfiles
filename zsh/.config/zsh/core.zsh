[[ -o interactive ]] || return

# Export
export PATH="$HOME/sdk/flutter/bin:$PATH"
export PATH=$HOME/.opencode/bin:$PATH
export CHROME_EXECUTABLE=chromium

# ──────────────────────────── GHCUP ────────────────────────────
[ -f "$HOME/.ghcup/env" ] && . "$HOME/.ghcup/env"

# ──────────────────────────── ZLE VI MODE ────────────────────────────
bindkey -v
KEYTIMEOUT=1   # 10ms: snappy Esc; multi-key vi seqs still work (burst input)

# ──────────────────────────── COMPINIT ────────────────────────────
# Runs after `bindkey -v` so completion key bindings land in the vi keymap,
# and before the tool integrations below so their compdef calls register.
# The slow security audit and fpath rescan only run when $fpath resolves to
# other directories (installed completions changed) or once a day; every
# other shell loads the existing dump directly.
if (( ! $+functions[compdef] )); then
  autoload -Uz compinit
  () {
    emulate -L zsh
    local dump=${ZDOTDIR:-$HOME}/.zcompdump stored=''
    local -a directories=( ${^fpath}(N-/:A) ) fresh=( $dump.fpath(N.mh-24) )
    local key="$ZSH_VERSION $directories"
    [[ -f $dump ]] && (( $#fresh )) && IFS= read -r stored < $dump.fpath
    if [[ $stored == "$key" ]]; then
      compinit -C -d $dump
      return
    fi
    compinit -d $dump || return
    # The compiled dump loads several times faster. Rename it into place so
    # concurrently starting shells never read a partial file.
    if zcompile -R -- $dump.$$.zwc $dump 2>/dev/null; then
      command mv -f -- $dump.$$.zwc $dump.zwc
    else
      command rm -f -- $dump.$$.zwc
    fi
    print -r -- $key 2>/dev/null >| $dump.fpath
  }
fi

# ──────────────────────────── Zoxide ────────────────────────────
source ~/.config/zsh/tool-init.zsh
__dotfiles_cached_init zoxide '' init zsh

# ──────────────────────────── OH MY POSH ────────────────────────────
# Not cached: each init must create a fresh prompt session.
if (( $+commands[oh-my-posh] )); then
  eval "$(oh-my-posh init zsh --config ~/.config/oh-my-posh/custom-respects-waybar.omp.json)"
  # Time commands with the shell's clock instead of starting oh-my-posh twice
  # per command. Hooks whose text changed upstream are left untouched.
  if zmodload zsh/datetime 2>/dev/null &&
     [[ $functions[_omp_preexec] == *'$($_omp_executable get millis)'* &&
        $functions[_omp_precmd] == *'$($_omp_executable get millis)'* ]]; then
    functions[_omp_preexec]=${functions[_omp_preexec]//'$($_omp_executable get millis)'/'${$(( EPOCHREALTIME * 1000 ))%.*}'}
    functions[_omp_precmd]=${functions[_omp_precmd]//'$($_omp_executable get millis)'/'${$(( EPOCHREALTIME * 1000 ))%.*}'}
  fi
fi
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

__dotfiles_cached_init atuin "${ATUIN_CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/atuin}/config.toml" \
  init zsh --disable-up-arrow --disable-ctrl-r --disable-ai
# Atuin's init prepends its strategy again whenever this file is re-sourced.
(( ${+ZSH_AUTOSUGGEST_STRATEGY} )) && typeset -gU ZSH_AUTOSUGGEST_STRATEGY
unfunction __dotfiles_cached_init

# ──────────────────────────── TEXINPUTS ────────────────────────────
export TEXINPUTS=$HOME/Desktop/acl-style-files-master//:

# Load optional version managers only when first used, and only if installed.
source ~/.config/zsh/lazy-tools.zsh

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
