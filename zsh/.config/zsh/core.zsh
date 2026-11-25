[[ -o interactive ]] || return

# Export
# Manually installed SDKs, when present; Nix provides both on NixOS.
path=( $HOME/.opencode/bin(N/) $HOME/sdk/flutter/bin(N/) $path )
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
# The full init script is cached so warm shells start no oh-my-posh process
# before the first prompt, and an upgrade regenerates it (oh-my-posh's own
# init cache keeps pointing at the old store path). Each shell still gets a
# fresh prompt session; every prompt names the config because no `init`
# stored it in that session. If upstream changes the script text this relies
# on, the cache falls back to a live `oh-my-posh init` on every start.
__dotfiles_omp_config=~/.config/oh-my-posh/custom-respects-waybar.omp.json
__dotfiles_omp_bake_init() {
  emulate -L zsh
  local script secondary config=${(qqqq)__dotfiles_omp_config}
  local session_pattern='export POSH_SESSION_ID="*";'
  local secondary_call='$($_omp_executable print secondary --shell=zsh)'
  local print_call='$_omp_executable print $type'
  local stream_call='"$_omp_executable" stream'
  local millis_call='$($_omp_executable get millis)'
  IFS= read -r -d '' script
  local -a lines=( "${(@f)script}" )
  if [[ -r /proc/sys/kernel/random/uuid && $lines[1] == ${~session_pattern} &&
        $script == *"$secondary_call"* && $script == *"$print_call "* ]] &&
     secondary=$("$commands[oh-my-posh]" print secondary --shell=zsh --config "$__dotfiles_omp_config"); then
    lines[1]='export POSH_SESSION_ID=$(</proc/sys/kernel/random/uuid)'
    script=${(F)lines}
    script=${script//"$secondary_call"/${(qqqq)secondary}}
    script=${script//"$print_call "/"$print_call --config=$config "}
    script=${script//"$stream_call"/"$stream_call --config=$config"}
    # Time commands with the shell's clock instead of starting oh-my-posh
    # twice per command.
    script=${script//"$millis_call"/'${$(( EPOCHREALTIME * 1000 ))%.*}'}
    print -r -- 'zmodload zsh/datetime'
    print -r -- "$script"
  else
    print -r -- "eval \"\$(oh-my-posh init zsh --config $config)\""
  fi
}
__dotfiles_cached_init --filter __dotfiles_omp_bake_init oh-my-posh "$__dotfiles_omp_config" \
  init zsh --print --config "$__dotfiles_omp_config"
unfunction __dotfiles_omp_bake_init
unset __dotfiles_omp_config

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
[[ -d $HOME/Desktop/acl-style-files-master ]] && export TEXINPUTS=$HOME/Desktop/acl-style-files-master//:

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
