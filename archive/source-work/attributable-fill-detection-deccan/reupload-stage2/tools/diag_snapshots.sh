#!/usr/bin/env bash
# Run the public diagnostics against every intermediate src snapshot of every attempt.
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
OUT=$R/analysis/snapshot_diag.txt; : > "$OUT"
for s in 01 02 03 04 05 06 07 08; do
  for d in "$R/replay/slot$s/snapshots"/*/; do
    n=$(basename "$d"); [ -d "$d/src" ] || continue
    line=$(bash "$R/tools/run_diag.sh" "snap_$s" "$d/src" "$R/pkg/environment/tests" 2>&1)
    summ=$(echo "$line" | grep -E "^snap_$s:" | sed "s/^snap_$s://")
    fails=$(echo "$line" | grep -oE 'test [a-z_]+ \.\.\. FAILED' | awk '{print $2}' | tr '\n' ' ')
    printf "slot%s %-34s %s\n    fails: %s\n" "$s" "$n" "$summ" "$fails" | tee -a "$OUT"
  done
done
echo "SNAPSHOT DIAG DONE" | tee -a "$OUT"
