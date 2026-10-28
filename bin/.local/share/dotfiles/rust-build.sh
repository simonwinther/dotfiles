# Shared build workflow, sourced by the Rust tools' *-build launchers.
# Resolve this through the checkout so each package can also build before Stow.
# Set source_root before rust_build_init for a tree outside .local/share/TOOL.

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
  # snapshot if copying fails. Compiler output stays in a separate target tree.
  local snapshot
  snapshot="$(mktemp -d "$cache_root/source.XXXXXX")"
  if ! cp -R "$source_root/." "$snapshot/"; then
    rm -rf -- "$snapshot"
    return 1
  fi
  rm -rf -- "$cache_root/build"
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
