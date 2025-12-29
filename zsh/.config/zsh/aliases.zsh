# ───────────── PATH ───────────── 
alias lg='lazygit'

# ───────────── Clipboard Copy ─────────────  
# For X11
if [[ "$XDG_SESSION_TYPE" == "wayland" ]]; then
    alias cc='wl-copy'
else
    alias cc='xclip -selection clipboard'
fi

# ───────────── Eza (better ls) ─────────────
alias ls="eza --icons=always"
alias ll="eza -la --icons=always"

# ──────────── Zoxide (better cd) ─────────────
alias cd="z"

# ──────────── Clear Command ─────────────
alias cl='clear'

# ─────────── Quick Exit ─────────────
alias q='exit'

# ─────────── Tmux Shortcuts ─────────────
alias ta="tmux attach"


# ─────────── fastfetch Shortcuts ─────────────
alias ff="fastfetch"
