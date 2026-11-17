# shellcheck shell=bash
# Shared build workflow, sourced by the Rust tools' *-build launchers.
# Resolve this through the checkout so each package can also build before Stow.
# Set source_root before rust_build_init for a tree outside .local/share/TOOL.

# Snapshots of read-only sources (the Nix store) keep their read-only modes.
rust_build_remove_tree() {
  [[ -e "$1" ]] || return 0
  chmod -R -- u+w "$1" && rm -rf -- "$1"
}

rust_build_init() {
  local tool="$1" script="$2"
  shift 2
  case "${1:-}" in
    ''|--check) ;;
    *) printf 'Usage: %s [--check]\n' "${script##*/}" >&2; return 2 ;;
  esac
  if (( $# > 1 )); then
    printf 'Usage: %s [--check]\n' "${script##*/}" >&2
    return 2
  fi
  build_check=false
  [[ "${1:-}" != --check ]] || build_check=true
  source_root="${source_root:-$(dirname "$(dirname "$script")")/share/$tool}"
  cache_root="${XDG_CACHE_HOME:-$HOME/.cache}/$tool"
  if [[ ! -f "$source_root/Cargo.lock" ]]; then
    printf '%s: Missing source Cargo.lock; update the dotfiles checkout.\n' "${script##*/}" >&2
    return 1
  fi
  umask 077
  mkdir -p "$cache_root/build" "$cache_root/bin"
  exec 9>"$cache_root/build.lock"
  flock -x 9

  # A complete snapshot removes deleted modules/tests and preserves the old
  # snapshot if copying fails. Compiler output stays in a separate target tree,
  # so an in-tree Cargo target directory is not copied. Under the lock, any
  # other snapshot is left over from an interrupted build.
  local snapshot entry
  local -a entries=()
  for snapshot in "$cache_root"/source.??????; do
    rust_build_remove_tree "$snapshot"
  done
  for entry in "$source_root"/* "$source_root"/.[!.]* "$source_root"/..?*; do
    [[ -e "$entry" || -L "$entry" ]] || continue
    [[ "${entry##*/}" == target && -d "$entry" ]] || entries+=("$entry")
  done
  snapshot="$(mktemp -d "$cache_root/source.XXXXXX")"
  if ! cp -R -- "${entries[@]}" "$snapshot/" || ! chmod -R -- u+w "$snapshot"; then
    rust_build_remove_tree "$snapshot"
    return 1
  fi
  rust_build_remove_tree "$cache_root/build"
  mv -- "$snapshot" "$cache_root/build"
  export CARGO_TARGET_DIR="$cache_root/target"
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
}

rust_build_cargo() {
  local subcommand="$1"
  shift
  nice -n 10 cargo "$subcommand" --locked --manifest-path "$cache_root/build/Cargo.toml" "$@"
}

rust_build_checks() {
  if "$build_check"; then
    nice -n 10 cargo fmt --manifest-path "$cache_root/build/Cargo.toml" -- --check
    rust_build_cargo clippy "$@" --all-targets -- -D warnings
    rust_build_cargo test "$@" --all-targets
  fi
}

rust_build_publish() {
  local binary="$1" destination="$2"
  install -m 755 "$binary" "$destination.new"
  mv -f -- "$destination.new" "$destination"
}
