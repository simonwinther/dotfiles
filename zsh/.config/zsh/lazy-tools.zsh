# NVM and SDKMAN define shell functions. Invoke those functions after loading;
# `command nvm` / `command sdk` would search for nonexistent executables instead.
export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"
export SDKMAN_DIR="${SDKMAN_DIR:-$HOME/.sdkman}"

if [[ -s "$NVM_DIR/nvm.sh" ]]; then
  __dotfiles_load_nvm() {
    unfunction nvm node npm npx
    source "$NVM_DIR/nvm.sh" || return
    if (( ! $+functions[nvm] )); then
      print -u2 -- 'NVM did not define its nvm function.'
      return 127
    fi
  }

  nvm()  { __dotfiles_load_nvm && nvm "$@"; }
  node() { __dotfiles_load_nvm && command node "$@"; }
  npm()  { __dotfiles_load_nvm && command npm "$@"; }
  npx()  { __dotfiles_load_nvm && command npx "$@"; }
fi

if [[ -s "$SDKMAN_DIR/bin/sdkman-init.sh" ]]; then
  sdk() {
    unfunction sdk
    source "$SDKMAN_DIR/bin/sdkman-init.sh" || return
    if (( ! $+functions[sdk] )); then
      print -u2 -- 'SDKMAN did not define its sdk function.'
      return 127
    fi
    sdk "$@"
  }
fi
