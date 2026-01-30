# ZSH reload with reminder
reload_zsh_with_reminder() {
  source ~/.zshrc
  zle -M "󱄊 Config reloaded. (Tip: Use 'ez' to edit and reload in one go!)"
}
zle -N reload_zsh_with_reminder
