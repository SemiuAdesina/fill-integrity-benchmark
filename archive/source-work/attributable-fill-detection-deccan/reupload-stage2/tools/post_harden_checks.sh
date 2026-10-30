#!/usr/bin/env bash
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2; A=$R/analysis/final5; mkdir -p "$A"; cd "$R"
bash tools/build_images.sh "$R/pkg" v5 > "$A/images.log" 2>&1 || { echo IMAGES FAILED; exit 1; }
bash tools/run_slots.sh "$R/pkg" v5 "$A/replay" "$R"/replay/slot0{1,7,8}/final/src > "$A/replay_run.log" 2>&1; cat "$A/replay/RESULTS.txt"
bash tools/exploit_controls.sh v5 "$A/controls.txt" > "$A/controls_run.log" 2>&1; cat "$A/controls.txt"
echo "POST HARDEN DONE"
