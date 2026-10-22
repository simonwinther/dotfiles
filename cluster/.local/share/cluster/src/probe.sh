# Read-only transport for tool discovery. Output is NUL-delimited records;
# version requirements and installation decisions live in Rust.
target_home=${CLUSTER_SETUP_HOME:-$HOME}
case $target_home in /*) ;; *) printf '%s\n' 'The target home must be an absolute path' >&2; exit 1 ;; esac
PATH="$target_home/.local/bin:$PATH"
export PATH
record() { printf '%s\0%s\0%s\0%s\0' "$1" "$2" "$3" "$4"; }
record platform 0 "$(hostname) ($(uname -sm))" ''
for name in nvim tmux git rg fd fzf tree-sitter curl unzip python3 stow; do
  flag=--version
  case $name in tmux) flag=-V ;; unzip) flag=-v ;; esac
  response=$("$name" "$flag" 2>/dev/null)
  code=$?
  location=$(command -v "$name" 2>/dev/null)
  if [ "$name" = stow ] && [ "$code" -ne 0 ] && [ -x "$target_home/.local/share/cluster-setup/stow/bin/stow" ]; then
    location="$target_home/.local/share/cluster-setup/stow/bin/stow"
    response=$("$location" --version 2>/dev/null)
    code=$?
  fi
  record "$name" "$code" "$response" "$location"
done
response=$(LC_ALL=C tmux -L default list-sessions 2>&1)
record tmux-server "$?" "$response" ''
compiler=''
for candidate in cc gcc clang; do
  if "$candidate" --version >/dev/null 2>&1; then compiler=$(command -v "$candidate"); break; fi
done
record compiler 0 "$compiler" ''
for name in perl make; do
  location=$(command -v "$name" 2>/dev/null)
  record "$name" "$?" '' "$location"
done
