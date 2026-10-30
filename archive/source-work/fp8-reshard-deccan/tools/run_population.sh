#!/usr/bin/env bash
# Run candidate source trees through the REAL verifier (Docker), one after another.
set -uo pipefail
W=~/benchmark-suite-work/fp8-reshard-deccan; OUT="$1"; shift
for d in "$@"; do
  name=$(basename "$d")
  res=$(RC_SKIP_BUILD=1 RC_AGENT_TAG=base RC_VER_TAG=base bash "$W/tools/run_control.sh" "$W/pkg" patched "$name" "$d" 2>&1)
  reward=$(echo "$res" | sed -n 's/.*REWARD=\([^ ]*\).*/\1/p'); fails=$(echo "$res" | sed -n 's/.*FAILS: \(.*\)$/\1/p')
  passed=$(echo "$res" | sed -n 's/.*verdict passed=\([^ ]*\).*/\1/p'); work=$(echo "$res" | sed -n 's/^WORKDIR: //p')
  note=$(echo "$res" | grep -o "patched build/run failed" || true)
  printf '%-40s reward=%s passed=%s %s fails=%s\n' "$name" "${reward:-?}" "${passed:-?}" "${note}" "${fails:-none}" >> "$OUT"
  rm -rf "$work"
done
echo DONE >> "$OUT"
