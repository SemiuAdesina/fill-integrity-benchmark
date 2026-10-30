#!/usr/bin/env bash
# Fresh extraction of the final ZIP: preflight, tree identity, cold (no-cache)
# image builds, oracle / NOP / hang from the extracted copy, diagnostics from the
# extracted environment against the reference and the starter.
set -uo pipefail
R=~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2
ZIP=~/Downloads/coroutine-frame-reuse-deccan-stage1-final.zip
OUT=$R/analysis/final_zip_cold_runs.txt; : > "$OUT"
[ -d "$R/fresh" ] && mv "$R/fresh" "$R/fresh-prev-$(date +%H%M%S)"
mkdir -p "$R/fresh"; unzip -q "$ZIP" -d "$R/fresh"
P="$R/fresh/coroutine-frame-reuse"
{ echo "zip sha256: $(shasum -a 256 "$ZIP" | cut -d' ' -f1)"; echo "extracted root: $(ls "$R/fresh")"; } | tee -a "$OUT"
echo "=== preflight on extraction ===" | tee -a "$OUT"; bash "$R/tools/preflight.sh" "$P" | tee -a "$OUT"
echo "=== diff extraction vs pkg ===" | tee -a "$OUT"; diff -r "$P" "$R/pkg" && echo "extraction identical to pkg" | tee -a "$OUT"
RC=~/benchmark-suite-work/coroutine-frame-reuse-deccan/tools/run_control.sh
echo "=== cold oracle (no-cache builds) ===" | tee -a "$OUT"
RC_BUILD_FLAGS=--no-cache bash "$RC" "$P" oracle final-cold-oracle 2>&1 | tee "$R/analysis/final_cold_oracle.log" | grep -E '^MODE' | tee -a "$OUT"
w=$(grep -o 'WORKDIR: .*' "$R/analysis/final_cold_oracle.log" | cut -d' ' -f2); head -3 "$w/logs/driver-summary.txt" | tee -a "$OUT"; echo "judge link: $(cat "$w/logs/verifier-link.txt")" | tee -a "$OUT"
echo "=== NOP (same cold images) ===" | tee -a "$OUT"
bash "$RC" "$P" nop final-cold-nop 2>&1 | tee "$R/analysis/final_cold_nop.log" | grep -E '^MODE' | tee -a "$OUT"
w=$(grep -o 'WORKDIR: .*' "$R/analysis/final_cold_nop.log" | cut -d' ' -f2); head -3 "$w/logs/driver-summary.txt" | tee -a "$OUT"
echo "=== hanging candidate via the cold verifier image ===" | tee -a "$OUT"
bash "$RC" "$P" patched final-cold-hang "$R/lab/xc/hang_in_step" 2>&1 | grep -E '^MODE' | tee -a "$OUT"
echo "=== diagnostics from the extracted environment ===" | tee -a "$OUT"
for cand in ref nop; do
  docker run --rm --network none -v "$P/environment:/src:ro" -v "$R/lab/$cand:/cand:ro" rc-agent:final-cold-oracle /bin/bash -c 'cp -R /src /app2 && cp /cand/runtime.hpp /app2/include/roundel/ && cp /cand/runtime.cpp /app2/src/ && cd /app2 && cmake -S . -B build -DCMAKE_BUILD_TYPE=Debug >/dev/null && cmake --build build -j 4 --target diagnostics >/dev/null 2>&1; for d in runtime_smoke lifecycle ticket_identity nested_coroutines scheduler_clock; do ./build/diag_$d | tail -1; done' 2>&1 | sed "s/^/$cand: /" | tee -a "$OUT"
done
echo "ALL COLD RUNS DONE" | tee -a "$OUT"
