open_pdf_fzf() {
  local file
  file=$(fd -e pdf -t f . "$HOME" | fzf) || return
  nohup zathura "$file" >/dev/null 2>&1 &
}

zle -N open_pdf_fzf
