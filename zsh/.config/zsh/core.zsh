# ──────────────────────────── GHCUP ────────────────────────────
[ -f "$HOME/.ghcup/env" ] && . "$HOME/.ghcup/env"

# ──────────────────────────── OH MY POSH ────────────────────────────
eval "$(oh-my-posh init zsh --config ~/.config/oh-my-posh/custom-respects-waybar.omp.json)"
#eval "$(oh-my-posh init zsh --config https://raw.githubusercontent.com/JanDeDobbeleer/oh-my-posh/refs/heads/main/themes/tokyo.omp.json)"

# ──────────────────────────── ZSH PLUGINS ────────────────────────────
if [[ "$XDG_CURRENT_DESKTOP" == "GNOME" ]]; then
    # GNOME system paths (if you want to keep your old paths)
    source /usr/share/zsh-autosuggestions/zsh-autosuggestions.zsh 2>/dev/null
    source /usr/share/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh 2>/dev/null
else
    # Arch correct paths
    source /usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh 2>/dev/null
    source /usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh 2>/dev/null
fi

# ──────────────────────────── HISTORY ────────────────────────────
HISTFILE=$HOME/.zhistory 
SAVEHIST=1000
HISTSIZE=999
setopt share_history
setopt hist_expire_dups_first
setopt hist_ignore_dups 
setopt hist_verify

# ──────────────────────────── TEXINPUTS ────────────────────────────
export TEXINPUTS=/home/simon/Desktop/acl-style-files-master//:

# ──────────────────────────── NVM ────────────────────────────
export NVM_DIR="$HOME/.nvm"
[ -s "$NVM_DIR/nvm.sh" ] && \. "$NVM_DIR/nvm.sh"  
[ -s "$NVM_DIR/bash_completion" ] && \. "$NVM_DIR/bash_completion"  

# ──────────────────────────── SDKMAN ────────────────────────────
export SDKMAN_DIR="$HOME/.sdkman"
[[ -s "$HOME/.sdkman/bin/sdkman-init.sh" ]] && source "$HOME/.sdkman/bin/sdkman-init.sh"
