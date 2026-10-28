# Cache shell integration in the local cache. Warm shells only source Zsh code;
# tool upgrades, changed arguments, and config edits regenerate it atomically.
zmodload -F zsh/stat b:zstat
__dotfiles_cached_init() {
  local tool="$1" dependency="$2"
  shift 2
  (( $+commands[$tool] )) || return 0
  local executable="${commands[$tool]:A}"
  local directory="${XDG_CACHE_HOME:-$HOME/.cache}/zsh"
  local cache="$directory/$tool-init.zsh"
  local -A binary_stat config_stat
  zstat -H binary_stat -- "$executable" || return
  [[ ! -f "$dependency" ]] || zstat -H config_stat -- "$dependency" || return
  local fingerprint="# $executable ${binary_stat[device]}:${binary_stat[inode]}:${binary_stat[size]}:${binary_stat[mtime]}:${binary_stat[ctime]} ${config_stat[inode]}:${config_stat[size]}:${config_stat[mtime]}:${config_stat[ctime]} ${(qq)@}" first_line=''
  [[ ! -r "$cache" ]] || IFS= read -r first_line < "$cache"
  if [[ "$first_line" != "$fingerprint" ]]; then
    local temporary="$cache.$$.tmp"
    if ! (
      umask 077
      mkdir -p -m 700 -- "$directory" && {
        print -r -- "$fingerprint"
        "$executable" "$@"
      } > "$temporary"
    ); then
      rm -f -- "$temporary"
      print -u2 -- "Could not initialize $tool."
      return 1
    fi
    mv -f -- "$temporary" "$cache" || return
  fi
  source "$cache"
}
