#!/usr/bin/env bash
input=$(cat)
cmd=$(jq -r '.tool_input.command // empty' <<<"$input")

case "$cmd" in
    cargo|"cargo "*|jj|"jj "*|vc-x1|"vc-x1 "*|"./target/debug/vc-x1 "*|"./target/release/vc-x1 "*)
    jq -n '{hookSpecificOutput:{hookEventName:"PreToolUse",
      permissionDecision:"allow",
      permissionDecisionReason:"trusted commands"}}'
    ;;
esac
exit 0
