# shx bash widget: fill the edit line, never run anything (T-606).
# Install: source /path/to/contrib/shell/shx.bash  (then Ctrl-G on a line)
_shx_fill() {
  local out code
  [ -z "$READLINE_LINE" ] && return
  out=$(shx --exit-on-risk "$READLINE_LINE")
  code=$?
  if [ "$code" -eq 0 ]; then
    READLINE_LINE=$out
    READLINE_POINT=${#READLINE_LINE}
  elif [ "$code" -eq 3 ]; then
    echo "shx: risky command withheld (exit 3)" >&2
  elif [ "$code" -eq 4 ]; then
    echo "shx: backend unavailable (exit 4)" >&2
  else
    echo "shx: no command (exit $code)" >&2
  fi
}
bind -x '"\C-g": _shx_fill'
