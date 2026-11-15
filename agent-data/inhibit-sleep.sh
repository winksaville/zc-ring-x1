#!/usr/bin/env bash
# usage: inhibit-sleep.sh start|stop|sub-start|sub-stop   (JSON from Claude Code on stdin)
#
# Holds a systemd idle inhibit for the session from the first prompt until the
# turn ends. Subagents outlive the turn that spawned them, so sub-start / sub-stop
# keep a count, and `stop` leaves the inhibit in place while any subagent runs.
# The turn that the last subagent's completion re-invokes ends with a normal
# `stop`, which then releases it.
set -euo pipefail
sid=$(jq -r .session_id)
run="${XDG_RUNTIME_DIR:-/tmp}"
pidfile="$run/cc-inhibit-$sid.pid"
subfile="$run/cc-inhibit-$sid.subs"

start() {
  [[ -f $pidfile ]] && kill -0 "$(<"$pidfile")" 2>/dev/null && return 0
  setsid systemd-inhibit --what=idle --mode=block --who=claude-code \
    --why="session $sid working" sleep infinity </dev/null &>/dev/null &
  echo $! >"$pidfile"
}
subs() { [[ -f $subfile ]] && cat "$subfile" || echo 0; }

case "$1" in
  start) start ;;
  sub-start)
    echo $(( $(subs) + 1 )) >"$subfile"
    start ;;
  sub-stop)
    n=$(( $(subs) - 1 )); (( n < 0 )) && n=0
    echo "$n" >"$subfile" ;;
  stop)
    (( $(subs) > 0 )) && exit 0   # a subagent is still working: hold the inhibit
    rm -f "$subfile"
    [[ -f $pidfile ]] && { kill -- -"$(<"$pidfile")" 2>/dev/null; rm -f "$pidfile"; } ;;
  end)
    rm -f "$subfile"
    [[ -f $pidfile ]] && { kill -- -"$(<"$pidfile")" 2>/dev/null; rm -f "$pidfile"; } ;;
esac
exit 0
