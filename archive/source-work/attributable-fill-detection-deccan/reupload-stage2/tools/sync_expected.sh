#!/usr/bin/env bash
# Copy a probe label's records into pkg/tests/expected, reporting what changed. sync_expected.sh <label>
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2; L="$1"; changed=0; new=0
for c in $(awk '/^graded /{print $2}' "$R/pkg/tests/cases.txt"); do
  if [ -f "$R/pkg/tests/expected/$c" ]; then cmp -s "$R/lab/probe/$L/results/$c" "$R/pkg/tests/expected/$c" || { echo "CHANGED: $c"; changed=$((changed+1)); }; else echo "NEW: $c"; new=$((new+1)); fi
  cp "$R/lab/probe/$L/results/$c" "$R/pkg/tests/expected/$c"
done
echo "changed=$changed new=$new total=$(ls "$R/pkg/tests/expected" | wc -l | tr -d ' ')"
