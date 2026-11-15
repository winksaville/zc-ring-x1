#!/usr/bin/env bash
# A Claude Code PreToolUse hook on Bash: denies the two ways a command's exit status is masked,
# per AGENTS.md > Working practices. Text matches, no parsing, so anything else is left alone.
input=$(cat)
cmd=$(jq -r '.tool_input.command // empty' <<<"$input")

form='Run the command under test bare and last in the invocation, since the Bash tool reports a nonzero exit; when piped, set -o pipefail first; to report and still fail, cmd || { rc=$?; echo failed=$rc; exit $rc; }.'

deny() {
  jq -n --arg r "$1 $form" '{hookSpecificOutput:{hookEventName:"PreToolUse",
    permissionDecision:"deny", permissionDecisionReason:$r}}'
  exit 0
}

# An echo of the status prints it while the invocation exits with the echo's 0.
if grep -Eq '(^|[^[:alnum:]_-])echo[[:space:]][^;&|]*(\$\?|\$\{\?\}|PIPESTATUS)' <<<"$cmd"; then
  deny 'Exit status masked: an echo of $? or PIPESTATUS.'
fi

# A cargo or vc-x1 command piped into a filter exits with the filter's status.
if ! grep -q pipefail <<<"$cmd" &&
  grep -Eq '(^|[;&(|])[[:space:]]*([^[:space:];&|]*/)?(cargo|vc-x1(-dev)?)([[:space:]][^|]*)?\|[[:space:]]*(tail|head|grep|rg)([[:space:]]|$)' <<<"$cmd"; then
  deny 'Exit status masked: a cargo or vc-x1 command piped into tail, head, grep, or rg without pipefail.'
fi
exit 0
