#!/bin/zsh

# Search history with up/down arrows based on current input
bindkey '5A' history-search-backward
bindkey '5B' history-search-forward

# ;f expands to | fzf
bindkey " " expand_fzf_space
bindkey "^M" expand_fzf_enter

# ───────────────── Grouping by Alt ───────────────── 

# <Alt + [g]o>: Trigger fuzzy `cd`
bindkey -s '^[g' 'gcd\n'

# <Alt + [r]un>: Search and run command from history
bindkey '^[r' fzf-history-widget

# <Alt + [v]im> Open nvim in current directory
bindkey -s '^[v' 'nvim .^M'

# <Alt + [s]ource>: Source .zshrc
bindkey -s '^[s' 'source ~/.zshrc^M'

#<Alt + [t]mux>: Start or attach to tmux session
bindkey -s '^[t' 'tmux new -A -s main\n'
