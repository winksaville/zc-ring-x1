#!/usr/bin/env bash
# Build a 4-revision substep ladder for manual squash inspection.
#
# State after this script:
#   bookmark "base" -> empty commit (no counter.txt)
#   r1   counter.txt = "line 1 count 0"
#   r2   counter.txt = "line 1 count 1"
#   r3   counter.txt = "line 1 count 2"
#   r4   counter.txt = "line 1 count 3"   (@ sits here)
#
# Then run squash recipes by hand from /tmp/substep-test, e.g.:
#
#   jj squash --from "base..@-" --into @                            # downward
#
#   jj squash --from "base..@" --into base --ignore-immutable       # upward
#   jj edit @-

set -euo pipefail

ROOT=/tmp/substep-test

rm -rf "$ROOT"
mkdir -p "$ROOT"
cd "$ROOT"

jj git init --colocate >/dev/null

#jj describe -m "base (empty)" >/dev/null
jj bookmark create base -r @
#jj new >/dev/null

echo "count 0" > counter.txt
jj describe -m "count 0" >/dev/null

jj new >/dev/null
echo "count 1" > counter.txt
jj describe -m "count 1" >/dev/null

jj new >/dev/null
echo "count 2" > counter.txt
jj describe -m "count 2" >/dev/null

jj new >/dev/null
echo "count 3" > counter.txt
jj describe -m "count 3" >/dev/null

echo
echo "::: cwd=$ROOT"
echo
jj log -r "all()"
echo "----- counter.txt -----"
cat counter.txt
