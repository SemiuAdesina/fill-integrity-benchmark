#!/usr/bin/env bash
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
for s in 01 02 03 04 05 06 07 08; do python3 "$R/tools/replay_in_container.py" "$s" afd-replay:base 2>&1 | tail -2; done
echo "REPLAY ALL DONE"
