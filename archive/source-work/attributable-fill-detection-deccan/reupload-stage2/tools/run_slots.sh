#!/usr/bin/env bash
# Grade the reference (oracle), the untouched project (nop) and every candidate src dir given.
#   run_slots.sh <task_root> <image_tag> <out_dir> [candidate src dirs...]
set -uo pipefail
ROOT="$1"; TAG="$2"; OUT="$3"; shift 3; mkdir -p "$OUT"
RC=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2/tools/run_control.sh
summ() { local reward; reward=$(sed -n 's/^MODE=.*REWARD=\([^ ]*\).*/\1/p' "$OUT/$1.log"); local tot; tot=$(sed -n 's/^MODE=.*\(TOTAL.*\)$/\1/p' "$OUT/$1.log"); local fails; fails=$(sed -n 's/^FAILS: //p' "$OUT/$1.log");
  printf "%-10s reward=%s %s fails: %s\n" "$1" "${reward:-?}" "${tot:-?}" "$fails" | tee -a "$OUT/RESULTS.txt"
  w=$(grep -o 'WORKDIR: .*' "$OUT/$1.log" | cut -d' ' -f2); cp "$w/logs/verdict.txt" "$OUT/$1.verdict.txt" 2>/dev/null; cp "$w/logs/run.log" "$OUT/$1.run.log" 2>/dev/null; cp "$w/logs/build.log" "$OUT/$1.build.log" 2>/dev/null; rm -rf "$w"; }
: > "$OUT/RESULTS.txt"
AFD_IMAGE_TAG="$TAG" bash "$RC" "$ROOT" oracle "$TAG-oracle" > "$OUT/oracle.log" 2>&1; summ oracle
AFD_IMAGE_TAG="$TAG" bash "$RC" "$ROOT" nop "$TAG-nop" > "$OUT/nop.log" 2>&1; summ nop
for d in "$@"; do s=$(basename "$(dirname "$d")"); [ "$(basename "$d")" = src ] || s=$(basename "$d"); AFD_IMAGE_TAG="$TAG" bash "$RC" "$ROOT" patched "$TAG-$s" "$d" > "$OUT/$s.log" 2>&1; summ "$s"; done
echo "RUN_SLOTS DONE" | tee -a "$OUT/RESULTS.txt"
