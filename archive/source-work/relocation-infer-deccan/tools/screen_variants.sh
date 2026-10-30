#!/usr/bin/env bash
# Native screening of candidate source trees: build, run on every fixture, compare to certified answers.
# usage: screen_variants.sh <out_file> <dir>...   (each dir holds recover.rs [+ recover/])
set -uo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan
OUT="$1"; shift
: > "$OUT"
for d in "$@"; do
  name=$(basename "$d")
  if ! bash "$W/tools/build_candidate.sh" "$name" "$d" > /tmp/screen_build_$name.log 2>&1; then
    printf '%-40s BUILD_FAIL\n' "$name" >> "$OUT"; continue
  fi
  line=""
  for f in public case_a case_b case_c case_d case_e case_f; do
    out=/tmp/screen_${name}_$f.json; rm -f "$out"
    if python3 "$W/tools/with_timeout.py" 120 "$W/lab/cand/$name/target/release/reloc-recover" --corpus "$W/pkg/tests/fixtures/$f/corpus" --out "$out" >/dev/null 2>&1 && [ -f "$out" ]; then
      if python3 "$W/tools/check_result.py" "$out" "$W/pkg/tests/fixtures/$f.expected.json" >/dev/null 2>&1; then line="$line $f:ok"; else line="$line $f:WRONG"; fi
    else line="$line $f:NO_OUTPUT"; fi
  done
  printf '%-40s%s\n' "$name" "$line" >> "$OUT"
done
echo DONE >> "$OUT"
