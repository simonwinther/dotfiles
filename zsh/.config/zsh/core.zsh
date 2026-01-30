# ────────────── ZSH CONFIG FILES ─────────────
source ~/.config/zsh/functions/init.zsh

# keybind widgets must be sourced 
for f in ~/.config/zsh/keybinds/*.zsh; do
  [[ "$f" == *"/init.zsh" ]] && continue
  source "$f"
done

source ~/.config/zsh/keybinds/init.zsh
source ~/.config/zsh/aliases.zsh

# ──────────────────────────── GHCUP ────────────────────────────
[ -f "$HOME/.ghcup/env" ] && . "$HOME/.ghcup/env"

# ──────────────────────────── Zoxide ────────────────────────────
eval "$(zoxide init zsh)"

# ──────────────────────────── OH MY POSH ────────────────────────────
eval "$(oh-my-posh init zsh --config ~/.config/oh-my-posh/custom-respects-waybar.omp.json)"
#eval "$(oh-my-posh init zsh --config https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/refs/heads/main/themes/tokyo.omp.json)"

# ──────────────────────────── ZSH PLUGINS ────────────────────────────
source /usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh 2>/dev/null
source /usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh 2>/dev/null

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
export TEXINPUTS=/home/simon/Desktop/acl-style-files-master//:

# ──────────────────────────── NVM ────────────────────────────
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
export SDKMAN_DIR="$HOME/.sdkman"

sdk() {
  unset -f sdk
  [[ -s "$SDKMAN_DIR/bin/sdkman-init.sh" ]] && source "$SDKMAN_DIR/bin/sdkman-init.sh"
  command sdk "$@"
}

#  ──────────────────────────── CURSOR FIX (Allow zsh to draw cursor in tmux) ────────────────────────────
_fix_cursor() {
  [[ -n "$TMUX" ]] || return
  echo -ne '\033[6 q'
}
typeset -ga precmd_functions
precmd_functions+=(_fix_cursor)# 

