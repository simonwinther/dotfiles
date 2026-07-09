# Native ZLE vi mode with a small prompt indicator.
typeset -g __dotfiles_zle_vi_mode_label='[I]'
typeset -gi __dotfiles_zle_vi_mode_pre_redraw_reset=0

# Cursor shapes per mode (DECSCUSR): 1 blink block, 2 steady block,
# 5 blink beam, 6 steady beam. Override these to taste.
typeset -g __dotfiles_zle_vi_mode_cursor_insert=$'\e[6 q'   # steady bar (your Ghostty has cursor-style-blink=false; a blinking bar \e[5 q falls back to block there)
typeset -g __dotfiles_zle_vi_mode_cursor_normal=$'\e[2 q'   # steady block
typeset -g __dotfiles_zle_vi_mode_cursor_visual=$'\e[2 q'   # steady block
typeset -g __dotfiles_zle_vi_mode_cursor_state=''           # last shape emitted

# Strip the [I]/[N]/[V] indicator from a line once it's accepted, so only the
# line you're editing shows it. Without this, every committed prompt freezes its
# RPROMPT into the scrollback and a resize leaves a stack of stale indicators.
setopt transient_rprompt

bindkey -M viins '^W' backward-kill-word

__dotfiles_zle_vi_mode_clear_postdisplay() {
  local suffix

  for suffix in ' [I]' ' [N]' ' [V]'; do
    if [[ ${POSTDISPLAY:-} == *"$suffix" ]]; then
      POSTDISPLAY="${POSTDISPLAY[1,$(( $#POSTDISPLAY - $#suffix ))]}"
      return 0
    fi
  done

  return 1
}

__dotfiles_zle_vi_mode_print_osc() {
  # Only emit terminal control sequences when stdout is a real terminal, so
  # cursor/color escapes never leak into pipes, files, or `zsh -i -c` capture.
  [[ -t 1 ]] || return
  local sequence="$1"

  print -rn -- "$sequence"
}

__dotfiles_zle_vi_mode_sync_cursor() {
  local label="${1:-[I]}"

  [[ $label == $__dotfiles_zle_vi_mode_cursor_state ]] && return
  __dotfiles_zle_vi_mode_cursor_state=$label

  case $label in
    '[V]')
      __dotfiles_zle_vi_mode_print_osc $'\e]12;#ffffff\a'
      __dotfiles_zle_vi_mode_print_osc "$__dotfiles_zle_vi_mode_cursor_visual"
      ;;
    '[N]')
      __dotfiles_zle_vi_mode_print_osc $'\e]112\a'
      __dotfiles_zle_vi_mode_print_osc "$__dotfiles_zle_vi_mode_cursor_normal"
      ;;
    *)
      __dotfiles_zle_vi_mode_print_osc $'\e]112\a'
      __dotfiles_zle_vi_mode_print_osc "$__dotfiles_zle_vi_mode_cursor_insert"
      ;;
  esac
}

__dotfiles_zle_vi_mode_attach_rprompt() {
  setopt prompt_subst

  local marker='${__dotfiles_zle_vi_mode_label}'
  local segment='%F{#94e2d5}${__dotfiles_zle_vi_mode_label}%f'

  [[ $RPROMPT == *${marker}* ]] && return 1

  RPROMPT="${RPROMPT:+${RPROMPT} }${segment}"
  return 0
}

__dotfiles_zle_vi_mode_set_label() {
  local keymap="${1:-${KEYMAP:-main}}"
  local label='[I]'

  if (( ${REGION_ACTIVE:-0} )); then
    label='[V]'
  elif [[ $keymap == vicmd ]]; then
    label='[N]'
  fi

  if [[ $__dotfiles_zle_vi_mode_label == "$label" ]]; then
    __dotfiles_zle_vi_mode_sync_cursor "$label"
    return 1
  fi

  __dotfiles_zle_vi_mode_label="$label"
  __dotfiles_zle_vi_mode_sync_cursor "$label"
  return 0
}

__dotfiles_zle_vi_mode_keymap_select() {
  local changed=0

  __dotfiles_zle_vi_mode_attach_rprompt && changed=1
  __dotfiles_zle_vi_mode_set_label "${KEYMAP:-main}" && changed=1

  (( changed )) && [[ -n ${WIDGET:-} ]] && zle reset-prompt
}

__dotfiles_zle_vi_mode_refresh_prompt() {
  local changed=0

  __dotfiles_zle_vi_mode_attach_rprompt && changed=1
  __dotfiles_zle_vi_mode_set_label "${1:-${KEYMAP:-main}}" && changed=1

  (( changed )) && [[ -n ${WIDGET:-} ]] && zle reset-prompt
}

__dotfiles_zle_vi_mode_line_init() {
  local changed=0

  zle -K viins
  REGION_ACTIVE=0
  __dotfiles_zle_vi_mode_attach_rprompt && changed=1
  __dotfiles_zle_vi_mode_set_label viins && changed=1

  (( changed )) && [[ -n ${WIDGET:-} ]] && zle reset-prompt
}

__dotfiles_zle_vi_mode_line_pre_redraw() {
  local changed=0

  # Visual mode keeps the vicmd keymap while only REGION_ACTIVE changes, so
  # this hook catches [V] state changes that zle-keymap-select cannot see.
  __dotfiles_zle_vi_mode_attach_rprompt && changed=1
  __dotfiles_zle_vi_mode_set_label "${KEYMAP:-main}" && changed=1

  if (( changed && ! __dotfiles_zle_vi_mode_pre_redraw_reset )); then
    __dotfiles_zle_vi_mode_pre_redraw_reset=1
    zle reset-prompt
    __dotfiles_zle_vi_mode_pre_redraw_reset=0
  fi
}

# These wrappers force an immediate prompt refresh when visual mode starts or
# ends; the pre-redraw hook above covers region changes from other widgets.
__dotfiles_zle_vi_mode_visual_mode() {
  zle .visual-mode
  __dotfiles_zle_vi_mode_refresh_prompt "${KEYMAP:-vicmd}"
}

__dotfiles_zle_vi_mode_visual_line_mode() {
  zle .visual-line-mode
  __dotfiles_zle_vi_mode_refresh_prompt "${KEYMAP:-vicmd}"
}

__dotfiles_zle_vi_mode_deactivate_region() {
  zle .deactivate-region
  __dotfiles_zle_vi_mode_refresh_prompt "${KEYMAP:-main}"
}

__dotfiles_zle_vi_mode_vi_cmd_mode() {
  zle .vi-cmd-mode
  __dotfiles_zle_vi_mode_refresh_prompt "${KEYMAP:-vicmd}"
}

__dotfiles_zle_vi_mode_precmd() {
  __dotfiles_zle_vi_mode_attach_rprompt
  # No >/dev/null here: set_label must be allowed to emit the insert-cursor
  # escape so the cursor returns to a beam after the previous command.
  __dotfiles_zle_vi_mode_set_label viins
}

__dotfiles_zle_vi_mode_preexec() {
  # Keep the insert (beam) cursor while a command runs, and force the next
  # prompt to re-assert it (state='' defeats the dedup) in case the command
  # left the cursor in another shape (e.g. a full-screen TUI like nvim).
  __dotfiles_zle_vi_mode_print_osc $'\e]112\a'
  __dotfiles_zle_vi_mode_print_osc "$__dotfiles_zle_vi_mode_cursor_insert"
  __dotfiles_zle_vi_mode_cursor_state=''
}

__dotfiles_zle_vi_mode_restore_autosuggestions() {
  local pair widget original

  for pair in \
    _zsh_autosuggest_modify:__dotfiles_zle_vi_mode_orig_modify \
    _zsh_autosuggest_accept:__dotfiles_zle_vi_mode_orig_accept \
    _zsh_autosuggest_execute:__dotfiles_zle_vi_mode_orig_execute \
    _zsh_autosuggest_partial_accept:__dotfiles_zle_vi_mode_orig_partial_accept; do
    widget="${pair%%:*}"
    original="${pair#*:}"

    (( $+functions[$original] )) || continue
    functions[$widget]=$functions[$original]
    unfunction "$original" 2>/dev/null
  done
}

autoload -Uz add-zsh-hook add-zle-hook-widget
zle -N __dotfiles_zle_vi_mode_keymap_select
zle -N __dotfiles_zle_vi_mode_line_init
zle -N __dotfiles_zle_vi_mode_line_pre_redraw
zle -N visual-mode __dotfiles_zle_vi_mode_visual_mode
zle -N visual-line-mode __dotfiles_zle_vi_mode_visual_line_mode
zle -N deactivate-region __dotfiles_zle_vi_mode_deactivate_region
zle -N vi-cmd-mode __dotfiles_zle_vi_mode_vi_cmd_mode

add-zle-hook-widget zle-keymap-select __dotfiles_zle_vi_mode_keymap_select
add-zle-hook-widget zle-line-init __dotfiles_zle_vi_mode_line_init
add-zle-hook-widget zle-line-pre-redraw __dotfiles_zle_vi_mode_line_pre_redraw

add-zsh-hook precmd __dotfiles_zle_vi_mode_precmd
add-zsh-hook preexec __dotfiles_zle_vi_mode_preexec
__dotfiles_zle_vi_mode_restore_autosuggestions
__dotfiles_zle_vi_mode_clear_postdisplay
__dotfiles_zle_vi_mode_attach_rprompt

# ──────────────── Redraw + cursor on terminal resize ────────────────
# The mode hooks only reset-prompt when the mode actually changed, so a bare
# SIGWINCH never repositioned the RPROMPT indicator. Force a clean redraw (and
# re-assert the cursor shape) on resize, chaining any earlier TRAPWINCH.
(( $+functions[TRAPWINCH] )) && \
  functions[__dotfiles_zle_vi_mode_prev_winch]=$functions[TRAPWINCH]

TRAPWINCH() {
  (( $+functions[__dotfiles_zle_vi_mode_prev_winch] )) && \
    __dotfiles_zle_vi_mode_prev_winch "$@"
  zle && {
    __dotfiles_zle_vi_mode_sync_cursor "$__dotfiles_zle_vi_mode_label"
    zle reset-prompt
  }
}

# ──────────────── Text objects: ci" di( ca[ yi{ vi) … ────────────────
# Wire zsh's built-in text-object widgets into the visual and operator-pending
# keymaps (the same idiom oh-my-zsh's vi-mode plugin uses).
autoload -Uz select-bracketed select-quoted
zle -N select-bracketed
zle -N select-quoted
() {
  local m c
  for m in visual viopp; do
    for c in {a,i}${(s..)^:-'()[]{}<>bB'}; do
      bindkey -M $m "$c" select-bracketed
    done
    for c in {a,i}\` {a,i}\' {a,i}\"; do
      bindkey -M $m "$c" select-quoted
    done
  done
}

# Visual-mode add-surround: select text, press S then a pair character.
# (Full vim-surround cs/ds/ys is intentionally out of scope here.)
__dotfiles_zle_vi_mode_add_surround() {
  local key open close lo hi
  read -k 1 key || return

  case $key in
    '('|')'|'b') open='(' close=')' ;;
    '['|']')     open='[' close=']' ;;
    '{'|'}'|'B') open='{' close='}' ;;
    '<'|'>')     open='<' close='>' ;;
    '"')  open='"'  close='"'  ;;
    "'")  open="'"  close="'"  ;;
    '`')  open='`'  close='`'  ;;
    *) zle .deactivate-region; return ;;
  esac

  (( lo = MARK < CURSOR ? MARK : CURSOR ))
  (( hi = MARK < CURSOR ? CURSOR : MARK )); (( hi++ ))
  BUFFER="${BUFFER[1,lo]}${open}${BUFFER[lo+1,hi]}${close}${BUFFER[hi+1,-1]}"
  (( CURSOR = lo ))
  zle .deactivate-region
  zle .vi-cmd-mode
}
zle -N __dotfiles_zle_vi_mode_add_surround
bindkey -M visual 'S' __dotfiles_zle_vi_mode_add_surround

# ──────────────── Yank to the system clipboard (Wayland) ────────────────
if (( $+commands[wl-copy] )); then
  __dotfiles_zle_vi_mode_vi_yank() {
    zle .vi-yank
    printf '%s' "$CUTBUFFER" | wl-copy 2>/dev/null
  }
  __dotfiles_zle_vi_mode_vi_yank_eol() {
    zle .vi-yank-eol
    printf '%s' "$CUTBUFFER" | wl-copy 2>/dev/null
  }
  __dotfiles_zle_vi_mode_vi_yank_whole_line() {
    zle .vi-yank-whole-line
    printf '%s' "$CUTBUFFER" | wl-copy 2>/dev/null
  }
  zle -N vi-yank            __dotfiles_zle_vi_mode_vi_yank
  zle -N vi-yank-eol        __dotfiles_zle_vi_mode_vi_yank_eol
  zle -N vi-yank-whole-line __dotfiles_zle_vi_mode_vi_yank_whole_line
fi

# Assert the insert-mode cursor for the first prompt.
__dotfiles_zle_vi_mode_sync_cursor '[I]'
