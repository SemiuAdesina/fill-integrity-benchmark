#!/usr/bin/env bash
# Hostile verifier audit: every control must leave reward 0 (or 1 only where noted as a correctness control).
set -uo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan; OUT=$W/lab/security.txt; : > "$OUT"
run() { # label mode dir [env...]
  local label="$1" mode="$2" dir="$3"; shift 3
  local res; res=$(env "$@" RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base bash "$W/tools/run_control.sh" "$W/pkg" "$mode" "$label" "$dir" 2>&1)
  local reward; reward=$(echo "$res" | sed -n 's/.*REWARD=\([^ ]*\).*/\1/p')
  local el; el=$(echo "$res" | sed -n 's/.*ELAPSED=\([^ ]*\).*/\1/p')
  local fatal; fatal=$(echo "$res" | grep -o 'FATAL [^F]*' | head -1)
  local passed; passed=$(echo "$res" | sed -n 's/.*verdict passed=\([^ ]*\).*/\1/p')
  local work; work=$(echo "$res" | sed -n 's/^WORKDIR: //p')
  local extra=""
  [ -f "$work/logs/run.log" ] && extra=$(grep -E 'reward re-established|produced no acceptable artifact|exceeded its budget|failing closed' "$work/logs/run.log" | head -2 | tr '\n' ';')
  printf '%-44s reward=%s passed=%s elapsed=%s %s %s\n' "$label" "${reward:-?}" "${passed:-?}" "${el:-?}" "${fatal}" "${extra}" >> "$OUT"
  rm -rf "$work"
}
for d in $W/security/controls/h0[1-8]* $W/security/controls/h1[0-4]* $W/security/controls/h16* $W/security/controls/h17*; do run "$(basename "$d")" patched "$d"; done
run prewritten_reward_1 nop "" RC_PRE_REWARD=1
run prewritten_reward_1_with_reference patched "$W/pkg/solution/fixed/src" RC_PRE_REWARD=1
for d in $W/security/overlays/*; do run "$(basename "$d")" overlay "$d"; done
# env manipulation: hostile PATH/RUSTC/CARGO env seen by the verifier container are scrubbed by test.sh; emulate by docker -e
res=$(RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base bash -c 'W=~/benchmark-suite-work/relocation-infer-deccan; WORK=$(mktemp -d /tmp/rc-XXXXXX); mkdir -p $WORK/app $WORK/logs; cid=$(docker create pc-reloc-agent:base); docker cp "$cid:/app/." "$WORK/app/" >/dev/null 2>&1; docker rm $cid >/dev/null; mkdir -p $WORK/app/bin; printf "#!/bin/sh\necho 1 > /logs/verifier/reward.txt\nexit 0\n" > $WORK/app/bin/cargo; chmod +x $WORK/app/bin/cargo; docker run --rm --network none -e PATH=/app/bin:/usr/local/cargo/bin:/usr/bin:/bin -e RUSTC=/app/bin/cargo -e RUSTC_WRAPPER=/app/bin/cargo -e CARGO_HOME=/app -e CARGO_BUILD_RUSTC=/app/bin/cargo -e RELOC_FIXTURE_DIR=/app -v $WORK/app:/app -v $WORK/logs:/logs/verifier pc-reloc-verifier:base >/dev/null 2>&1; echo "reward=$(cat $WORK/logs/reward.txt) $(grep -E "^verdict" $WORK/logs/run.log)"; rm -rf $WORK')
printf '%-44s %s\n' "hostile_environment_variables" "$res" >> "$OUT"
# hang with the production probe budget (120 s) and build bomb with a 60 s build budget copy of test.sh
run h09_hang patched "$W/security/controls/h09_hang"
sed 's/^BUILD_TIMEOUT=600/BUILD_TIMEOUT=60/' "$W/pkg/tests/test.sh" > /tmp/test_build60.sh
run h15_macro_bomb_compile_failure patched "$W/security/controls/h15_build_bomb"
run h18_slow_const_build_budget_60s patched "$W/security/controls/h18_slow_const_build" RC_TEST_SH=/tmp/test_build60.sh
echo DONE >> "$OUT"
