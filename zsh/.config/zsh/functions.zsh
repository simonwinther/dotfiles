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
fzf-history-widget() {
  local selected
  selected=$(history 1 | tac | fzf | sed 's/^[ ]*[0-9]*[ ]*//')
  [[ -n $selected ]] && {
    BUFFER="$selected"
    zle accept-line
  }
}

zle -N fzf-history-widget

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
