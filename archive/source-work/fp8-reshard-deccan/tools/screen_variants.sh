#!/usr/bin/env bash
# Native screening: build each candidate dir, run on every fixture, compare to certified answers.
set -uo pipefail
W=~/benchmark-suite-work/fp8-reshard-deccan; OUT="$1"; shift; : > "$OUT"
FX=$(ls $W/author/fixtures)
for d in "$@"; do
  name=$(basename "$d")
  if ! bash "$W/tools/build_candidate.sh" "$name" "$d" > /tmp/fp8screen_$name.log 2>&1; then printf '%-40s BUILD_FAIL\n' "$name" >> "$OUT"; continue; fi
  line=""
  for f in $FX; do
    out=/tmp/fp8screen_${name}_$f.json; rm -f "$out"
    if python3 "$W/../relocation-infer-deccan/tools/with_timeout.py" 60 "$W/lab/cand/$name/fp8-restore" --checkpoint "$W/author/fixtures/$f/checkpoint.json" --target "$W/author/fixtures/$f/target.json" --out "$out" && [ -f "$out" ]; then
      if python3 "$W/tools/check_result.py" "$out" "$W/author/fixtures/$f/expected.json" >/dev/null 2>&1; then line="$line $f:ok"; else line="$line $f:WRONG"; fi
    else line="$line $f:NO_OUTPUT"; fi
  done
  printf '%-40s%s\n' "$name" "$line" >> "$OUT"
done
echo DONE >> "$OUT"
