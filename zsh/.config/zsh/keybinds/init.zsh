#!/bin/zsh

# Search history with up/down arrows based on current input
bindkey '5A' history-search-backward
bindkey '5B' history-search-forward

# ;f expands to | fzf
bindkey " " expand_fzf_space
bindkey "^M" expand_fzf_enter

# ───────────────── Grouping by Alt ───────────────── 
# <Alt + [f]ind>: Trigger fuzzy `cd` within the current git repo
bindkey -s '^[f' 'rcd\n'

# <Alt + Shift + [F]ind>: Trigger fuzzy `cd` from $HOME
bindkey -s '^[F' 'gcd\n'

# <Alt + [r]un>: Search and run command from history
bindkey '^[r' fzf-history-logic

# <Alt + [v]im> Open nvim 
bindkey -s '^[v' 'nvim\n'

# <Alt + [s]ource>: Source zsh config with reminder
bindkey '^[s' reload_zsh_with_reminder

#<Alt + [t]mux>: Start or attach to tmux session
bindkey -s '^[t' 'tmux new -A -s main\n'

# Alt + h/l to move by word
bindkey '^[h' backward-word
bindkey '^[l' forward-word

# Ctrl + Left/Right to move by word
bindkey "^[[1;5C" forward-word 
bindkey "^[[1;5D" backward-word

# Alt + o opens opencode 
bindkey -s '^[o' 'opencode\n'
