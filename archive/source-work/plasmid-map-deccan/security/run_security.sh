#!/usr/bin/env bash
# Hostile controls through the real verifier. Every line must end with reward=0.
set -uo pipefail
W=~/benchmark-suite-work/plasmid-map-deccan; OUT=$W/lab/final/security.txt; : > "$OUT"
export RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base
run() { local label="$1"; shift; local res; res=$("$@" 2>&1); local reward passed work note
  reward=$(echo "$res" | sed -n 's/.*REWARD=\([^ ]*\).*/\1/p'); passed=$(echo "$res" | sed -n 's/.*verdict passed=\([^ ]*\).*/\1/p'); work=$(echo "$res" | sed -n 's/^WORKDIR: //p')
  note=$(grep -E 'reward reset|exceeded its budget|FATAL|no acceptable artifact|did not build' "$work/logs/run.log" 2>/dev/null | head -1 | cut -c1-90)
  printf '%-44s reward=%s passed=%s %s\n' "$label" "${reward:-?}" "${passed:-?}" "$note" >> "$OUT"; rm -rf "$work"; }
for c in $W/security/controls/*/; do n=$(basename "$c"); run "$n" bash $W/tools/run_control.sh $W/pkg patched "$n" "$c"; done
for o in $W/security/overlays/*/; do n=$(basename "$o"); run "$n" bash $W/tools/run_control.sh $W/pkg overlay "$n" "$o"; done
run prewritten_reward_1 env RC_PRE_REWARD=1 bash $W/tools/run_control.sh $W/pkg nop prewritten
run prewritten_reward_1_with_reference env RC_PRE_REWARD=1 bash $W/tools/run_control.sh $W/pkg oracle prewritten_ref
# a verifier whose graded account is unavailable must fail closed
sed 's|^RUNUSER=/usr/sbin/runuser|RUNUSER=/nonexistent/runuser|' $W/pkg/tests/test.sh > /tmp/pm_test_norunuser.sh
run no_runuser_reference_must_fail_closed env RC_TEST_SH=/tmp/pm_test_norunuser.sh bash $W/tools/run_control.sh $W/pkg oracle norunuser
echo DONE >> "$OUT"
