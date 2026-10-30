#!/usr/bin/env bash
set -uo pipefail
W=~/benchmark-suite-work/reduction-plan-deccan; OUT="$1"; shift; : > "$OUT"; FX=$(ls $W/author/fixtures)
for d in "$@"; do name=$(basename "$d")
  if ! bash "$W/tools/build_candidate.sh" "$name" "$d" > /tmp/rpscreen_$name.log 2>&1; then printf '%-40s BUILD_FAIL\n' "$name" >> "$OUT"; continue; fi
  line=""
  for f in $FX; do out=/tmp/rpscreen_${name}_$f.json; rm -f "$out"
    if python3 ~/benchmark-suite-work/relocation-infer-deccan/tools/with_timeout.py 60 "$W/lab/cand/$name/reduction-recover" --problem "$W/author/fixtures/$f/problem.json" --out "$out" >/dev/null 2>&1 && [ -f "$out" ]; then
      if python3 "$W/tools/check_result.py" "$out" "$W/author/fixtures/$f/expected.json" >/dev/null 2>&1; then line="$line $f:ok"; else line="$line $f:WRONG"; fi
    else line="$line $f:NO_OUTPUT"; fi
  done; printf '%-40s%s\n' "$name" "$line" >> "$OUT"; done; echo DONE >> "$OUT"
