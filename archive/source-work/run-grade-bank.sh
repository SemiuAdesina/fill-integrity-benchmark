#!/usr/bin/env bash
set -uo pipefail
TASK="${1:?taskdir}"
CAND="${2:?candidate recover.cpp}"
LABEL="${3:-run}"
WS="$TASK/tests/workspace"
B=$(mktemp -d /tmp/bankgrade.XXXXXX)
trap 'rm -rf "$B"' EXIT
CXX=/usr/bin/clang++
FLAGS="-std=c++20 -O0 -Wall -Wextra -Wpedantic -Werror -I$WS/frozen/include -I$WS/shared"
mkdir -p "$B/res" "$B/lres"
$CXX $FLAGS "$WS/probe/main.cpp" "$CAND" "$WS/frozen/src/model.cpp" -o "$B/probe" 2>"$B/probe.err" || {
  echo "LABEL=$LABEL BUILD=fail"; sed -n 1,25p "$B/probe.err"; exit 0; }
$CXX $FLAGS "$WS/probe/main.cpp" "$WS/liar/recover.cpp" "$WS/frozen/src/model.cpp" -o "$B/liar" 2>>"$B/probe.err" || {
  echo "LABEL=$LABEL BUILD=liar-fail"; exit 0; }
$CXX $FLAGS "$WS/judge/main.cpp" "$WS/frozen/src/model.cpp" -o "$B/judge" 2>>"$B/probe.err" || {
  echo "LABEL=$LABEL BUILD=judge-fail"; sed -n 1,25p "$B/probe.err"; exit 0; }
CASES=($(sed -n '/^CASES=(/,/)/p' "$TASK/tests/test.sh" | tr '()' '  ' | sed 's/CASES=//'))
POP=${#CASES[@]}
CONTROL=0; "$B/judge" --selftest "$B/ctl" >"$B/ctl.log" 2>&1 && CONTROL=1
for c in "${CASES[@]}"; do "$B/liar" "$c" "$B/lres/$c" >/dev/null 2>&1; done
REG=0; "$B/judge" --grade "$B/lres" "$B/lv.txt" >"$B/lg.log" 2>&1 || REG=1
for c in "${CASES[@]}"; do "$B/probe" "$c" "$B/res/$c" >/dev/null 2>&1; done
GRC=0; "$B/judge" --grade "$B/res" "$B/v.txt" >"$B/g.log" 2>&1 || GRC=$?
TOTAL=$(awk '/^TOTAL /{print $2}' "$B/v.txt" 2>/dev/null); FAILED=$(awk '/^TOTAL /{print $4}' "$B/v.txt" 2>/dev/null)
: "${TOTAL:=0}"; : "${FAILED:=0}"
REWARD=0
[ "$GRC" -eq 0 ] && [ "$TOTAL" -eq "$POP" ] && [ "$FAILED" -eq 0 ] && [ "$CONTROL" -eq 1 ] && [ "$REG" -eq 1 ] && REWARD=1
echo "LABEL=$LABEL POPULATION=$POP TOTAL=$TOTAL FAILED=$FAILED CONTROL=$CONTROL REGRESSION=$REG REWARD=$REWARD"
grep '^FAIL ' "$B/g.log" 2>/dev/null | sed 's/^/  /'
exit 0
