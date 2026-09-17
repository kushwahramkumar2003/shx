# shx zsh widget: fill the edit buffer, never run anything (T-606).
# Install: source /path/to/contrib/shell/shx.zsh  (then Ctrl-G on a line)
_shx_widget() {
  local out code
  [ -z "$BUFFER" ] && return
  out=$(shx --exit-on-risk "$BUFFER")
  code=$?
  if (( code == 0 )); then
    BUFFER=$out
    CURSOR=${#BUFFER}
  elif (( code == 3 )); then
    zle -M "shx: risky command withheld (exit 3)"
  elif (( code == 4 )); then
    zle -M "shx: backend unavailable (exit 4)"
  else
    zle -M "shx: no command (exit $code)"
  fi
}
zle -N _shx_widget
bindkey '^G' _shx_widget
