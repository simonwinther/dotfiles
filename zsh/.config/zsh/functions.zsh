#  ───────────────── FZF ───────────────── 
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

# ───────────────── NVIM ───────────────── 
function v() {
  if [ $# -eq 0 ]; then 
    nvim .
  else
    nvim "$@"
  fi
}

# ───────────────── ZSH ─────────────────
# Edit and reload zsh config
ez() {
  nvim ~/.zshrc
  source ~/.zshrc
  print "󱄊 Zsh configuration reloaded."
}

# ZSH reload with reminder
reload_zsh_with_reminder() {
  source ~/.zshrc
  zle -M "󱄊 Config reloaded. (Tip: Use 'ez' to edit and reload in one go!)"
}
zle -N reload_zsh_with_reminder

# ───────────────── FZF UTILS ───────────────── 
# Global fuzzy CD from $HOME
gcd() {
    local dir
    dir=$(fd --base-directory "$HOME" --type d --hidden --follow --exclude .git \
        | fzf --height=60% --layout=reverse --border \
              --prompt="   gcd> " \
              --preview "eza --color=always --icons --group-directories-first -1 $HOME/{}" \
              --preview-window="right,50%")
    [[ -n "$dir" ]] && cd "$HOME/$dir"
}

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
    --color="prompt:4,pointer:2,hl:3,hl+:3,border:7,header:italic:4" \
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


# Fuzzy find file and open in nvim
vf() {
  local file
  file=$(fd --type f --hidden --exclude .git | fzf \
    --height=60% \
    --layout=reverse \
    --border \
    --prompt="   Open File> " \
    --preview '[[ -f {} ]] && (bat --style=numbers --color=always {} || cat {})')
    
  [[ -n "$file" ]] && nvim "$file"
}
# ───────────────── TLDRF ───────────────── 
# Too Long; Didn't Read [the] Flags: Explains a specific command string.
# Usage: tldrf <command> (e.g., 'tldrf tar -xvf archive.tar.gz')
tldrf () {
  curl -Gs "https://www.mankier.com/api/explain/?cols=$(tput cols)" --data-urlencode "q=$*"
}
