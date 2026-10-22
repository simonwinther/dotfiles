# Interactive defaults shared with non-Nix distributions through Stow.
[[ $- != *i* ]] && return
for session_file in "$HOME/.nix-profile/etc/profile.d/hm-session-vars.sh" "/etc/profiles/per-user/${USER}/etc/profile.d/hm-session-vars.sh"; do
  [[ ! -f $session_file ]] || source "$session_file"
done
unset session_file
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/bash"
for component in envs shell aliases; do source "$config_dir/$component"; done
for component in "$config_dir"/functions/*; do [[ ! -f $component ]] || source "$component"; done
source "$config_dir/init"
bind -f "$config_dir/inputrc"
unset config_dir component
