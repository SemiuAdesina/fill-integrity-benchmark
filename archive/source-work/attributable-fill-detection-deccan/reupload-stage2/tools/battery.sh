#!/usr/bin/env bash
# Grade every battery variant through the real verifier. battery.sh <image_tag> <out_file>
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
TAG="$1"; OUT="$2"; : > "$OUT"
for d in "$R"/lab/battery/*/; do
  v=$(basename "$d")
  AFD_IMAGE_TAG="$TAG" bash "$R/tools/run_control.sh" "$R/pkg" patched "$TAG-$v" "$d/src" > "$R/lab/battery/$v/run.log" 2>&1
  reward=$(sed -n 's/^MODE=.*REWARD=\([^ ]*\).*/\1/p' "$R/lab/battery/$v/run.log"); tot=$(sed -n 's/^MODE=.*\(TOTAL.*\)$/\1/p' "$R/lab/battery/$v/run.log"); fails=$(sed -n 's/^FAILS: //p' "$R/lab/battery/$v/run.log" | head -c 400)
  printf "%-46s reward=%s %s fails: %s\n" "$v" "${reward:-?}" "${tot:-?}" "$fails" | tee -a "$OUT"
  w=$(grep -o 'WORKDIR: .*' "$R/lab/battery/$v/run.log" | cut -d' ' -f2); rm -rf "$w"
done
echo "BATTERY DONE" | tee -a "$OUT"
