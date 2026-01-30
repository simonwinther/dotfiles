# Fuzzy search history 
fzf-history-logic() {
  local selected
  local preview_cmd="echo {}"
  if command -v bat &> /dev/null; then
    preview_cmd="echo {} | bat --color=always --style=plain --language=bash"
  fi

  # We use --expect to detect if the user pressed 'ctrl-y' or 'enter'
  selected=$(history 1 | tac | fzf \
    --height=80% \
    --layout=reverse \
    --border=rounded \
    --margin=1 \
    --padding=1 \
    --prompt="   History > " \
    --header="  RET: Exec  󰅍  C-y: Edit " \
    --header-first \
    --color="prompt:4,pointer:2,hl:2,hl+:2,border:7,header:italic:4" \
    --preview="$preview_cmd" \
    --preview-window="up:2:wrap:border-bottom" \
    --expect="ctrl-y") # Tells fzf to return the key pressed as the first line

  if [[ -n $selected ]]; then
    # fzf with --expect returns the key on the first line and the choice on the second
    local key=$(echo "$selected" | sed -n '1p')
    local cmd=$(echo "$selected" | sed -n '2p' | sed 's/^[ ]*[0-9]*[ ]*//')

    BUFFER="$cmd"
    CURSOR=$#BUFFER

    # If the first line is NOT 'ctrl-y', it means they hit Enter
    if [[ "$key" != "ctrl-y" ]]; then
      zle accept-line
    else
      zle reset-prompt
    fi
    return 0
  fi
  return 1
}
zle -N fzf-history-logic
