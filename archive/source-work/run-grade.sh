#!/usr/bin/env bash
# Local reimplementation of the verifier's grading logic for pre-upload audit.
# Mirrors tests/test.sh: probe records raw values, judge owns expected values,
# reward = judge-clean AND population-complete AND control-ok AND regression-ok.
# The privilege drop / chmod steps of the real verifier are container-only and
# are not reproduced here; grading semantics are identical.
set -uo pipefail

TASKDIR="$1"          # e.g. ~/benchmark-suite-work/convex/convex-recover
CANDIDATE="$2"        # path to a lib.rs to install as the candidate
LABEL="${3:-run}"

WS_SRC="$TASKDIR/tests/workspace"
[ -d "$WS_SRC" ] || { echo "no workspace at $WS_SRC"; exit 2; }
[ -f "$CANDIDATE" ] || { echo "no candidate at $CANDIDATE"; exit 2; }

RUN=$(mktemp -d "/tmp/grade-XXXXXXXX")
if [ -z "${KEEP:-}" ]; then trap 'rm -rf "$RUN"' EXIT; else echo "RUNDIR=$RUN"; fi
cp -r "$WS_SRC" "$RUN/ws"
cp "$CANDIDATE" "$RUN/ws/probe/src/candidate.rs"

export CARGO_TARGET_DIR="$RUN/target"
export CARGO_TERM_COLOR=never

if ! cargo build --offline --locked --workspace --manifest-path "$RUN/ws/Cargo.toml" > "$RUN/build.log" 2>&1; then
  if ! cargo build --workspace --manifest-path "$RUN/ws/Cargo.toml" > "$RUN/build.log" 2>&1; then
    echo "LABEL=$LABEL BUILD=FAIL REWARD=0"
    tail -30 "$RUN/build.log"
    exit 0
  fi
fi

PROBE="$RUN/target/debug/probe"
LIAR="$RUN/target/debug/liar_probe"
JUDGE="$RUN/target/debug/judge"
for b in "$PROBE" "$LIAR" "$JUDGE"; do
  [ -x "$b" ] || { echo "LABEL=$LABEL MISSING_ARTIFACT=$b"; exit 2; }
done

# Case population, read out of the verifier-owned case list.
CASES=$(grep -A400 -E 'pub const (CANDIDATE_)?CASES' "$RUN/ws/model/src/cases.rs" \
        | sed -n '1,/\];/p' | grep -oE '"[a-z0-9_]+"' | tr -d '"')
POP=$(echo "$CASES" | grep -c .)

mkdir -p "$RUN/results" "$RUN/liar-results"

# Record filename convention, taken from the task's own test.sh so the local
# harness stays faithful to the verifier it is standing in for.
SUFFIX=$(sed -n '/run_probe(){/,/^}/p' "$TASKDIR/tests/test.sh" \
         | grep -oE '"\$2/\$3[^"]*"' | head -1 | sed 's|"\$2/\$3||; s|"$||')

for c in $CASES; do
  "$PROBE" "$c" "$RUN/results/$c$SUFFIX" > "$RUN/probe.$c.out" 2>&1
  echo "probe $c exit=$? (not used for grading)" >> "$RUN/probe.log"
done

grade_rc=0
"$JUDGE" --grade "$RUN/results" "$RUN/verdict.txt" > "$RUN/grade.out" 2>&1 || grade_rc=$?
TOTAL=0; FAILED=0
if [ -f "$RUN/verdict.txt" ]; then
  read -r TOTAL FAILED < <(awk '/^TOTAL /{print $2, $4}' "$RUN/verdict.txt")
fi
: "${TOTAL:=0}"; : "${FAILED:=0}"

control_ok=0
"$JUDGE" --selftest "$RUN/control" > "$RUN/control.out" 2>&1 && control_ok=1

regression_ok=0
for c in $CASES; do "$LIAR" "$c" "$RUN/liar-results/$c$SUFFIX" > "$RUN/liar.$c.out" 2>&1; done
if "$JUDGE" --grade "$RUN/liar-results" "$RUN/liar-verdict.txt" > "$RUN/liar.out" 2>&1; then
  regression_ok=0
else
  regression_ok=1
fi

REWARD=0
if [ "$grade_rc" -eq 0 ] && [ "$TOTAL" -eq "$POP" ] && [ "$FAILED" -eq 0 ] \
   && [ "$control_ok" -eq 1 ] && [ "$regression_ok" -eq 1 ]; then
  REWARD=1
fi

echo "LABEL=$LABEL POPULATION=$POP TOTAL=$TOTAL FAILED=$FAILED CONTROL=$control_ok REGRESSION=$regression_ok REWARD=$REWARD"
if [ "$FAILED" != "0" ]; then
  grep '^FAIL' "$RUN/verdict.txt" | head -25
fi
