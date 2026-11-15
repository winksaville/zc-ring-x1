#!/usr/bin/env bash
# usage: hooks.sh check|install|test
#
# The Claude Code hooks the agent-files carry, beside this script, installed into
# ~/.claude/hooks and registered in ~/.claude/settings.json (AGENTS.md > Working practices).
#
#   check    each hook installed identical to this copy and registered, else what is not, exit 1
#   install  copy the hooks in and register what is missing, the settings file backed up first
#   test     run bash-guard.sh over commands it must deny or allow
#
# HOOKS_DIR and SETTINGS override the two destinations, for trying install somewhere harmless.
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
dest=${HOOKS_DIR:-$HOME/.claude/hooks}
settings=${SETTINGS:-$HOME/.claude/settings.json}
# How a registration names its script: the ~ form settings.json already uses, or the override.
cmd_dir=${HOOKS_DIR:-'~/.claude/hooks'}

scripts=(bash-allow.sh bash-guard.sh inhibit-sleep.sh)

# One registration per line: event, matcher (empty for none), command.
registrations() {
  cat <<EOF
UserPromptSubmit||$cmd_dir/inhibit-sleep.sh start
SubagentStart||$cmd_dir/inhibit-sleep.sh sub-start
SubagentStop||$cmd_dir/inhibit-sleep.sh sub-stop
Stop||$cmd_dir/inhibit-sleep.sh stop
SessionEnd||$cmd_dir/inhibit-sleep.sh end
PreToolUse|Bash|$cmd_dir/bash-allow.sh
PreToolUse|Bash|$cmd_dir/bash-guard.sh
EOF
}

registered() { # event matcher command
  [[ -f $settings ]] || return 1
  jq -e --arg e "$1" --arg m "$2" --arg c "$3" \
    '(.hooks[$e] // []) | any(.[]; (.matcher // "") == $m and any(.hooks[]?; .command == $c))' \
    "$settings" >/dev/null
}

check() {
  local bad=0 s e m c
  for s in "${scripts[@]}"; do
    if [[ ! -f $dest/$s ]]; then
      echo "hooks: missing    $dest/$s"
      bad=1
    elif ! cmp -s "$here/$s" "$dest/$s"; then
      echo "hooks: differs    $dest/$s"
      bad=1
    fi
  done
  while IFS='|' read -r e m c; do
    if ! registered "$e" "$m" "$c"; then
      echo "hooks: unregistered $e${m:+ [$m]}: $c"
      bad=1
    fi
  done < <(registrations)
  if ((bad)); then
    echo "hooks: not installed, run: bash $here/hooks.sh install"
    return 1
  fi
  echo "hooks: ${#scripts[@]} hooks installed and registered"
}

install() {
  local s e m c tmp
  mkdir -p "$dest"
  for s in "${scripts[@]}"; do
    if ! cmp -s "$here/$s" "$dest/$s" 2>/dev/null; then
      cp "$here/$s" "$dest/$s.new"
      chmod +x "$dest/$s.new"
      mv "$dest/$s.new" "$dest/$s"
      echo "hooks: installed  $dest/$s"
    fi
  done
  [[ -f $settings ]] || echo '{}' >"$settings"
  local backed_up=0
  while IFS='|' read -r e m c; do
    registered "$e" "$m" "$c" && continue
    if ((!backed_up)); then
      cp "$settings" "$settings.$(date -u +%Y%m%dT%H%M%SZ).bak"
      backed_up=1
    fi
    tmp=$(mktemp "$settings.XXXXXX")
    jq --arg e "$e" --arg m "$m" --arg c "$c" '
      {type: "command", command: $c} as $h
      | .hooks //= {} | .hooks[$e] //= []
      | if any(.hooks[$e][]; (.matcher // "") == $m)
        then .hooks[$e] |= map(if (.matcher // "") == $m then .hooks += [$h] else . end)
        else .hooks[$e] += [(if $m == "" then {} else {matcher: $m} end) + {hooks: [$h]}]
        end' "$settings" >"$tmp"
    mv "$tmp" "$settings"
    echo "hooks: registered $e${m:+ [$m]}: $c"
  done < <(registrations)
  check
}

# Each case: the decision bash-guard.sh must make, then the command.
guard_cases() {
  cat <<'EOF'
deny|false; echo rc=$?
deny|vc-x1-dev install --profile dev 2>&1 | tail -4; echo rc=${PIPESTATUS[0]}
deny|cargo test 2>&1 | tail -3
deny|cargo test | tail -3; exit ${PIPESTATUS[0]}
deny|cd /home/me/vc-x1; vc-x1 validate | grep FAILED
deny|tmp/candidate/vc-x1 land -s b --dry-run | head
allow|jq -n 1 | grep 1
allow|cargo --version || { rc=$?; echo failed=$rc; exit $rc; }
allow|set -o pipefail; cargo test 2>&1 | tail -3
allow|cd /home/me/vc-x1; jq . f.json | grep x
allow|cargo test -- --include-ignored
allow|vc-x1 validate
allow|echo done
EOF
}

test_guard() {
  local want cmd out got fail=0 n=0
  while IFS='|' read -r want cmd; do
    out=$(jq -n --arg c "$cmd" '{tool_name: "Bash", tool_input: {command: $c}}' |
      bash "$here/bash-guard.sh")
    got=allow
    if [[ -n $out ]]; then
      got=$(jq -r '.hookSpecificOutput.permissionDecision' <<<"$out")
    fi
    n=$((n + 1))
    if [[ $got != "$want" ]]; then
      echo "hooks: FAIL want $want, got $got: $cmd"
      fail=1
    fi
  done < <(guard_cases)
  if ((fail)); then
    return 1
  fi
  echo "hooks: bash-guard.sh passed $n cases"
}

case "${1:-}" in
  check) check ;;
  install) install ;;
  test) test_guard ;;
  *)
    echo "usage: hooks.sh check|install|test" >&2
    exit 2
    ;;
esac
