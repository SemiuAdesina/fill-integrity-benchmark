#!/usr/bin/env bash
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
CASES="$*"
bash "$R/tools/probe_cases.sh" ref "$R/lab/ref/src" $CASES
bash "$R/tools/probe_cases.sh" nop "$R/lab/nop/src" $CASES
for s in 01 02 03 04 05 06 07 08; do bash "$R/tools/probe_cases.sh" slot$s "$R/replay/slot$s/final/src" $CASES; done
python3 "$R/tools/compare_probe.py" ref nop slot01 slot02 slot03 slot04 slot05 slot06 slot07 slot08 -- $CASES
echo "PROBE ALL DONE"
