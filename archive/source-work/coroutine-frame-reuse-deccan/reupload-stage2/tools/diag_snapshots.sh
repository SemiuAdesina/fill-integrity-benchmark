#!/usr/bin/env bash
# Build and run the public diagnostics against every intermediate snapshot.
W=~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2
OUT="$W/lab/diag_snap"; mkdir -p "$OUT"
run_one() { # <label> <cand_dir>
  docker run --rm --network none -v "$W/pkg/environment:/src:ro" -v "$2:/cand:ro" rc-agent:reup-base /bin/bash -c '
    cp -R /src /app2 && cp /cand/runtime.hpp /app2/include/roundel/ && cp /cand/runtime.cpp /app2/src/ && cd /app2
    cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug >/dev/null 2>&1
    if ! cmake --build build -j 4 --target diagnostics >/tmp/b.log 2>&1; then echo "BUILD FAILED"; grep -m3 "error" /tmp/b.log; exit 0; fi
    for d in runtime_smoke lifecycle ticket_identity nested_coroutines scheduler_clock; do timeout 20 ./build/diag_$d > /tmp/o.txt 2>&1; rc=$?; grep -E "^$d:" /tmp/o.txt | sed "s/\$/ exit=$rc/"; grep "FAIL" /tmp/o.txt | sed "s/^/    /"; done' > "$OUT/$1.txt" 2>&1
  echo "$1: $(grep -E 'passed|BUILD' "$OUT/$1.txt" | tr '\n' ' ')"
}
run_one nop "$W/lab/nop"; run_one ref "$W/lab/ref"
for s in 01 02 03 05 06 07 08; do
  for d in "$W/replay/slot$s/snapshots"/*/; do n=$(basename "$d"); case "$n" in 000_stepstart*) continue;; esac; run_one "slot${s}_$n" "$d"; done
done
echo "DIAG SNAPSHOTS DONE"
