for session_file in "$HOME/.nix-profile/etc/profile.d/hm-session-vars.sh" "/etc/profiles/per-user/${USER}/etc/profile.d/hm-session-vars.sh"; do
  [[ -f "$session_file" ]] && source "$session_file"
done
unset session_file
export PATH="$HOME/.local/bin:/usr/local/bin:$PATH"
export EZA_CONFIG_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/desktop/theme"
typeset -U path PATH # keep PATH unique 

if [[ -z ${DOTFILES:-} ]]; then
  typeset dotfiles_zshenv_source="${${(%):-%x}:A}"
  export DOTFILES="${dotfiles_zshenv_source:h:h}"
  unset dotfiles_zshenv_source
fi
