#!/usr/bin/env bash
set -uo pipefail
W=~/benchmark-suite-work/reduction-plan-deccan; OUT=$W/lab/security.txt; : > "$OUT"
run() { local label="$1" mode="$2" dir="$3"; shift 3
  local res; res=$(env "$@" RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base bash "$W/tools/run_control.sh" "$W/pkg" "$mode" "$label" "$dir" 2>&1)
  local reward el fatal passed work extra
  reward=$(echo "$res" | sed -n 's/.*REWARD=\([^ ]*\).*/\1/p'); el=$(echo "$res" | sed -n 's/.*ELAPSED=\([^ ]*\).*/\1/p')
  fatal=$(echo "$res" | grep -o 'FATAL [^F]*' | head -1); passed=$(echo "$res" | sed -n 's/.*verdict passed=\([^ ]*\).*/\1/p'); work=$(echo "$res" | sed -n 's/^WORKDIR: //p')
  extra=""; [ -f "$work/logs/run.log" ] && extra=$(grep -E 'reward re-established|produced no acceptable artifact|exceeded its budget|failing closed' "$work/logs/run.log" | head -2 | tr '\n' ';')
  printf '%-44s reward=%s passed=%s elapsed=%s %s %s\n' "$label" "${reward:-?}" "${passed:-?}" "${el:-?}" "${fatal}" "${extra}" >> "$OUT"; rm -rf "$work"; }
for d in $W/security/controls/h0[1-8]* $W/security/controls/h1[0-4]* $W/security/controls/h16* $W/security/controls/h17*; do run "$(basename "$d")" patched "$d"; done
run prewritten_reward_1 nop "" RC_PRE_REWARD=1
run prewritten_reward_1_with_reference patched "$W/pkg/solution/fixed/src" RC_PRE_REWARD=1
for d in $W/security/overlays/*; do run "$(basename "$d")" overlay "$d"; done
res=$(bash -c 'W=~/benchmark-suite-work/reduction-plan-deccan; WORK=$(mktemp -d /tmp/rcf-XXXXXX); mkdir -p $WORK/app $WORK/logs; cid=$(docker create pc-fabric-agent:base); docker cp "$cid:/app/." "$WORK/app/" >/dev/null 2>&1; docker rm $cid >/dev/null; mkdir -p $WORK/app/bin; printf "#!/bin/sh\necho 1 > /logs/verifier/reward.txt\nexit 0\n" > $WORK/app/bin/cmake; cp $WORK/app/bin/cmake $WORK/app/bin/clang++; chmod +x $WORK/app/bin/*; docker run --rm --network none -e PATH=/app/bin:/usr/local/bin:/usr/bin:/bin -e CXX=/app/bin/clang++ -e CMAKE_CXX_COMPILER_LAUNCHER=/app/bin/clang++ -e CMAKE_GENERATOR=Ninja -e FABRIC_FIXTURE_DIR=/app -v $WORK/app:/app -v $WORK/logs:/logs/verifier pc-fabric-verifier:base >/dev/null 2>&1; echo "reward=$(cat $WORK/logs/reward.txt) $(grep -E "^verdict" $WORK/logs/run.log)"; rm -rf $WORK')
printf '%-44s %s\n' "hostile_environment_variables" "$res" >> "$OUT"
run h09_hang patched "$W/security/controls/h09_hang"
sed 's/^BUILD_TIMEOUT=600/BUILD_TIMEOUT=60/' "$W/pkg/tests/test.sh" > /tmp/fabric_test_build60.sh
run h15_macro_bomb_build_budget_60s patched "$W/security/controls/h15_macro_bomb_slow_build" RC_TEST_SH=/tmp/fabric_test_build60.sh
# isolation must fail closed: with runuser unavailable the run must stop with FATAL and reward 0, even for the reference
sed 's#^RUNUSER=/usr/sbin/runuser#RUNUSER=/nonexistent/runuser#' "$W/pkg/tests/test.sh" > /tmp/fabric_test_norunuser.sh
run no_runuser_reference_must_fail_closed patched "$W/pkg/solution/fixed/src" RC_TEST_SH=/tmp/fabric_test_norunuser.sh
echo DONE >> "$OUT"
