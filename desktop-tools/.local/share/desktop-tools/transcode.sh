#!/usr/bin/env bash
set -euo pipefail

choose_value() {
  local title=$1; shift
  printf '%s\n' "$@" | desktopctl launcher --dmenu --placeholder "$title"
}
pick_media() {
  local title=$1 paths=$2 extensions=$3
  local -a directories
  IFS=: read -r -a directories <<< "$paths"
  fd --type f --ignore-case --regex "\.(${extensions// /|})$" "${directories[@]}" |
    desktopctl launcher --dmenu --placeholder "$title"
}
usage() {
  cat <<'EOF'
Usage:
  desktopctl transcode [--path path] [input] [format] [resolution]

With no input, pick a picture or video with Walker from ~/Pictures and ~/Videos,
or only from --path when provided. Then pick the output format and resolution.

Options:
  --path path  Limit interactive fuzzy file selection to this path

Formats:
  Pictures: jpg, png
  Videos: mp4, gif

Resolutions:
  Pictures: high, medium, low
  Videos: 4k, 1080p, 720p

Ctrl+C, SIGTERM, or SIGHUP cancels the encoder and removes temporary output.
Completed files never replace an existing output.
EOF
}

media_type() {
  local mime
  mime=$(file -b --mime-type "$1")

  case "$mime" in
  image/*) echo picture ;;
  video/*) echo video ;;
  *)
    echo "Unsupported file type: $mime" >&2
    return 1
    ;;
  esac
}

run_encoder() {
  if command -v desktop-transcode-native >/dev/null 2>&1; then
    exec desktop-transcode-native "$@"
  fi
  local binary="${XDG_CACHE_HOME:-$HOME/.cache}/desktop-transcode/bin/desktop-transcode-native"
  if [[ ! -x $binary ]]; then
    printf '%s\n' 'desktop-transcode: Run desktop-transcode-build to compile the Rust transcoder.' >&2
    return 1
  fi
  # Replace the picker shell so signals sent to its PID reach the supervisor.
  exec "$binary" "$@"
}

main() {
  local input="" format="" resolution="" search_path=""
  local type positional=()

  while (( $# > 0 )); do
    case "$1" in
    --path)
      shift
      [[ $# -gt 0 ]] || { echo "Missing value for --path" >&2; return 2; }
      search_path="$1"
      ;;
    --help | -h)
      usage
      return 0
      ;;
    --)
      shift
      positional+=("$@")
      break
      ;;
    --*)
      echo "Unknown option: $1" >&2
      usage >&2
      return 2
      ;;
    *)
      positional+=("$1")
      ;;
    esac
    shift
  done

  (( ${#positional[@]} <= 3 )) || { usage >&2; return 2; }
  input="${positional[0]:-}"
  format="${positional[1]:-}"
  resolution="${positional[2]:-}"
  search_path="${search_path:-$HOME/Pictures:$HOME/Videos}"

  if [[ -z $input ]]; then
    input=$(pick_media "Transcode picture or video" "$search_path" "jpg jpeg png webp gif heic avif mp4 mov m4v mkv webm avi")
  fi

  [[ -n $input ]] || return 1
  [[ -f $input ]] || { echo "File not found: $input" >&2; return 1; }

  type=$(media_type "$input")

  if [[ -z $format ]]; then
    if [[ $type == "picture" ]]; then
      format=$(choose_value "Select format" jpg png)
    else
      format=$(choose_value "Select format" mp4 gif)
    fi
  fi

  if [[ -z $resolution ]]; then
    if [[ $type == "picture" ]]; then
      resolution=$(choose_value "Select resolution" high medium low)
    else
      resolution=$(choose_value "Select resolution" 4k 1080p 720p)
    fi
  fi

  [[ -n $format && -n $resolution ]] || return 130
  run_encoder "$type" "$input" "$format" "$resolution"
}

main "$@"
