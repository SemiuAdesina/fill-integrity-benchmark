#!/usr/bin/env bash
# Run the verifier on every reconstructed final implementation (patched mode),
# plus the reference (oracle) and the untouched project (nop).
#   run_slots.sh <task_root> <tag> <out_dir> [slot dirs...]
set -uo pipefail
ROOT="$1"; TAG="$2"; OUT="$3"; shift 3
mkdir -p "$OUT"
RC=~/benchmark-suite-work/coroutine-frame-reuse-deccan/tools/run_control.sh
summ() { # <label> <workdir>
  local w; w=$(grep -o 'WORKDIR: .*' "$OUT/$1.log" | cut -d' ' -f2)
  cp "$w/logs/driver-summary.txt" "$OUT/$1.summary.txt" 2>/dev/null
  cp "$w/logs/driver.log" "$OUT/$1.driver.log" 2>/dev/null
  local reward; reward=$(sed -n 's/^MODE=.*REWARD=\([^ ]*\).*/\1/p' "$OUT/$1.log")
  local passed; passed=$(sed -n 's/^passed=//p' "$OUT/$1.summary.txt" 2>/dev/null)
  local graded; graded=$(sed -n 's/^graded=//p' "$OUT/$1.summary.txt" 2>/dev/null)
  local fails; fails=$(grep '^FAIL ' "$OUT/$1.summary.txt" 2>/dev/null | awk '{print $2}' | tr '\n' ' ')
  printf "%-10s reward=%s passed=%s/%s fails: %s\n" "$1" "${reward:-?}" "${passed:-?}" "${graded:-?}" "$fails" | tee -a "$OUT/RESULTS.txt"
}
: > "$OUT/RESULTS.txt"
bash "$RC" "$ROOT" oracle "$TAG-oracle" > "$OUT/oracle.log" 2>&1; summ oracle
bash "$RC" "$ROOT" nop "$TAG-nop" > "$OUT/nop.log" 2>&1; summ nop
for d in "$@"; do
  s=$(basename "$d"); [ "$s" = final ] && s=$(basename "$(dirname "$d")")
  bash "$RC" "$ROOT" patched "$TAG-$s" "$d" > "$OUT/$s.log" 2>&1; summ "$s"
done
echo "RUN_SLOTS DONE" | tee -a "$OUT/RESULTS.txt"
