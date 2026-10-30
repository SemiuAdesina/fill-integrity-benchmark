#!/usr/bin/env bash
# Probe a candidate src against all existing graded cases plus the lab cases and compare.
#   check_ref.sh <label> <src_dir>
set -uo pipefail
cd ~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
LABEL="$1"; SRC="$2"
ALL=$(grep -E '^graded ' pkg/tests/cases.txt | awk '{print $2}' | tr '\n' ' ')
H="h1_a_recycled_identifier_names_the_order_that_holds_it_now h2_a_block_naming_a_recycled_identifier_divides_over_the_order_that_holds_it_now h3_an_entry_leg_whose_acknowledgement_was_lost_is_recovered_under_the_same_reference h4_a_block_that_closes_a_resting_leg_leaves_a_real_gap_to_cover h5_a_block_after_partial_fills_finalizes_the_orders_it_allocates_nothing_to"
bash tools/probe_cases.sh "$LABEL" "$SRC" $ALL $H 2>&1 | tail -1
echo "=== existing 32 vs pkg expected ==="; ok=0; bad=0
for c in $ALL; do if cmp -s "lab/probe/$LABEL/results/$c" "pkg/tests/expected/$c"; then ok=$((ok+1)); else bad=$((bad+1)); echo "DIFF: $c"; fi; done
echo "identical=$ok differing=$bad"
echo "=== new cases: $LABEL vs consensus ==="
for pair in "h1:slot01" "h2:slot02" "h3:slot07" "h4:slot01" "h5:slot01"; do c=${pair%%:*}; s=${pair##*:}; full=$(echo $H | tr ' ' '\n' | grep "^$c")
  if cmp -s "lab/probe/$LABEL/results/$full" "lab/probe/$s/results/$full"; then echo "$c: $LABEL == $s"; else echo "$c: $LABEL != $s"; python3 tools/compare_probe.py "$LABEL" "$s" -- "$full" | tail -3; fi; done
