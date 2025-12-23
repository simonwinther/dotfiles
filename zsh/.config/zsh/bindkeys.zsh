#!/bin/zsh

# Search history with up/down arrows based on current input
bindkey '5A' history-search-backward
bindkey '5B' history-search-forward

# ;f expands to | fzf
bindkey " " expand_fzf_space
bindkey "^M" expand_fzf_enter

# Alt + g to trigger fuzzy `cd`
bindkey -s '^[g' 'gcd\n'

# Alt + r to search and run command from history
bindkey '^[r' fzf-history-widget



