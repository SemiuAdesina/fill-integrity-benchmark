#!/usr/bin/env bash
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
bash "$R/tools/run_diag.sh" nop "$R/lab/nop/src" 2>&1 | grep -E '^nop:|\[.*\] test .*FAILED'
for s in 01 02 03 04 05 06 07 08; do bash "$R/tools/run_diag.sh" slot$s "$R/replay/slot$s/final/src" 2>&1 | grep -E "^slot$s:|\] test .*FAILED"; done
echo "DIAG ALL DONE"
