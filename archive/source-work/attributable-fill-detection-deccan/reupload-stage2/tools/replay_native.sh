#!/usr/bin/env bash
# Old-population replay against the final graded set, natively. replay_native.sh <out_file>
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2; OUT="$1"; : > "$OUT"
ALL=$(awk '/^graded /{print $2}' "$R/pkg/tests/cases.txt" | tr '\n' ' ')
for s in 01 02 03 04 05 06 07 08; do
  bash "$R/tools/probe_cases.sh" "final_slot$s" "$R/replay/slot$s/final/src" $ALL >/dev/null 2>&1
  fails=""; n=0
  for c in $ALL; do n=$((n+1)); cmp -s "$R/lab/probe/final_slot$s/results/$c" "$R/pkg/tests/expected/$c" || fails="$fails $c"; done
  nf=$(echo $fails | wc -w | tr -d ' ')
  printf "slot%s graded_failed=%s/%s reward=%s fails:%s\n" "$s" "$nf" "$n" "$([ "$nf" = 0 ] && echo 1 || echo 0)" "$fails" | tee -a "$OUT"
done
echo "REPLAY NATIVE DONE" | tee -a "$OUT"
