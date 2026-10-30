#!/usr/bin/env bash
# Final regression on the frozen package: images, oracle, nop, 8-slot replay, battery, controls.
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2; TAG=v4; A=$R/analysis/final
mkdir -p "$A"; cd "$R"
bash tools/build_images.sh "$R/pkg" $TAG > "$A/images.log" 2>&1 || { echo "IMAGE BUILD FAILED"; cat "$A/images.log"; exit 1; }
echo "images built" 
bash tools/run_slots.sh "$R/pkg" $TAG "$A/replay" "$R"/replay/slot0{1,2,3,4,5,6,7,8}/final/src > "$A/replay_run.log" 2>&1
cat "$A/replay/RESULTS.txt"
bash tools/battery.sh $TAG "$A/battery.txt" > "$A/battery_run.log" 2>&1
bash tools/exploit_controls.sh $TAG "$A/controls.txt" > "$A/controls_run.log" 2>&1
echo "FINAL REGRESSION DONE"
