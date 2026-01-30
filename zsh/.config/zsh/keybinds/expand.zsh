# ; fzf cmd
expand_fzf_space() {
  if [[ $LBUFFER == *";f" ]]; then
    LBUFFER="${LBUFFER%";f"}| fzf"
  fi
  zle self-insert
}
zle -N expand_fzf_space
# ;fzf cmd 
expand_fzf_enter() {
  if [[ $LBUFFER == *";f" ]]; then
    LBUFFER="${LBUFFER%";f"}| fzf"
  fi
  zle accept-line
}
zle -N expand_fzf_enter

