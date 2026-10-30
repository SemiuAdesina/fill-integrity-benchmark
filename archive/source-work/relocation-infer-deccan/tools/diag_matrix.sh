#!/usr/bin/env bash
# Build each snapshot natively, run it on the public corpus, and tabulate the public diagnostic groups.
set -uo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan
printf '%-34s %-9s %-9s %-9s %-9s %-9s %-9s %-9s\n' snapshot corpus schema scalar field pair cross replay
for d in "$@"; do
  name=$(basename "$d")
  bash "$W/tools/build_candidate.sh" "snap_$name" "$d" > /tmp/snapbuild_$name.log 2>&1 || { printf '%-34s BUILD_FAIL\n' "$name"; continue; }
  out=/tmp/snap_$name.json; rm -f "$out"
  python3 "$W/tools/with_timeout.py" 120 "$W/lab/cand/snap_$name/target/release/reloc-recover" --corpus "$W/pkg/environment/corpus" --out "$out" >/dev/null 2>&1
  res=$("$W/lab/cand/snap_$name/target/release/reloc-diag" --corpus "$W/pkg/environment/corpus" --result "$out" 2>/dev/null | grep -E '^GROUP' | awk '{print $3}' | tr '\n' ' ')
  printf '%-34s %s\n' "$name" "$res"
done
