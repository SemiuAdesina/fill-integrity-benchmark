#!/usr/bin/env bash
# Fresh extraction of the final ZIP: preflight, tree identity, cold (no-cache) image builds,
# oracle / NOP / hang from the extracted copy, public diagnostics from the extracted environment,
# and a replay spot-check of slots 07 and 08 against the cold verifier.
set -uo pipefail
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
ZIP=~/Downloads/attributable-fill-detection-deccan-stage1-final.zip
OUT=$R/analysis/final_zip_cold_runs.txt; : > "$OUT"
[ -d "$R/fresh" ] && mv "$R/fresh" "$R/fresh-prev-$(date +%H%M%S)"
mkdir -p "$R/fresh"; unzip -q "$ZIP" -d "$R/fresh"
P="$R/fresh/attributable-fill-detection"
{ echo "$(date '+%H:%M:%S')"; echo "zip sha256: $(shasum -a 256 "$ZIP" | cut -d' ' -f1)"; echo "extracted root: $(ls "$R/fresh")"; } | tee -a "$OUT"
echo "=== preflight on extraction ===" | tee -a "$OUT"; bash "$R/tools/preflight.sh" "$P" | grep -E 'FAIL|failures|words' | tee -a "$OUT"
echo "=== diff extraction vs pkg ===" | tee -a "$OUT"; diff -r "$P" "$R/pkg" && echo "extraction identical to pkg" | tee -a "$OUT"
echo "=== cold image builds (no cache) ===" | tee -a "$OUT"
docker build --no-cache -t afd-agent:cold "$P/environment" > "$R/analysis/cold_agent_build.log" 2>&1 && echo "agent image built" | tee -a "$OUT" || { echo "AGENT IMAGE FAILED" | tee -a "$OUT"; tail -20 "$R/analysis/cold_agent_build.log"; }
docker build --no-cache -t afd-verifier:cold "$P/tests" > "$R/analysis/cold_verifier_build.log" 2>&1 && echo "verifier image built" | tee -a "$OUT" || { echo "VERIFIER IMAGE FAILED" | tee -a "$OUT"; tail -20 "$R/analysis/cold_verifier_build.log"; }
RC=$R/tools/run_control.sh
echo "=== fresh oracle ===" | tee -a "$OUT"; AFD_IMAGE_TAG=cold bash "$RC" "$P" oracle cold-oracle 2>&1 | tee "$R/analysis/cold_oracle.log" | grep -E '^MODE|^RUNLOG' | tee -a "$OUT"
w=$(grep -o 'WORKDIR: .*' "$R/analysis/cold_oracle.log" | cut -d' ' -f2); grep -E 'timing|reward reset|CONTROL' "$w/logs/run.log" | tee -a "$OUT"; grep -c PASS "$w/logs/verdict.txt" | sed 's/^/verdict PASS lines: /' | tee -a "$OUT"; rm -rf "$w"
echo "=== fresh NOP ===" | tee -a "$OUT"; AFD_IMAGE_TAG=cold bash "$RC" "$P" nop cold-nop 2>&1 | tee "$R/analysis/cold_nop.log" | grep -E '^MODE' | tee -a "$OUT"; w=$(grep -o 'WORKDIR: .*' "$R/analysis/cold_nop.log" | cut -d' ' -f2); rm -rf "$w"
echo "=== fresh hang (candidate loops forever) ===" | tee -a "$OUT"; AFD_IMAGE_TAG=cold bash "$RC" "$P" patched cold-hang "$R/lab/xc/hang/src" 2>&1 | tee "$R/analysis/cold_hang.log" | grep -E '^MODE|^RUNLOG' | tee -a "$OUT"; w=$(grep -o 'WORKDIR: .*' "$R/analysis/cold_hang.log" | cut -d' ' -f2); grep -E 'exceeded|failing closed|reward reset' "$w/logs/run.log" | head -3 | tee -a "$OUT"; rm -rf "$w"
echo "=== fresh replay spot-check (07, 08, 01) ===" | tee -a "$OUT"
for s in 07 08 01; do AFD_IMAGE_TAG=cold bash "$RC" "$P" patched "cold-slot$s" "$R/replay/slot$s/final/src" 2>&1 | grep -E '^MODE|^FAILS' | sed "s/^/slot$s /" | tee -a "$OUT"; done
echo "=== public diagnostics from the extracted environment ===" | tee -a "$OUT"
for cand in ref nop; do src="$R/lab/ref2/src"; [ "$cand" = nop ] && src="$P/environment/src"
  docker run --rm --network none -v "$P/environment:/env:ro" -v "$src:/cand:ro" afd-agent:cold /bin/bash -c 'cp -R /env /app2 && rm -rf /app2/src && cp -R /cand /app2/src && cd /app2 && for g in observation_identity direct_progress block_reconciliation neutralization_lifecycle restart_recovery cross_mandate_isolation; do r=$(cargo test --offline --test $g -- --test-threads=1 2>&1 | grep -E "^test result" | tail -1); echo "$g: $r"; done' 2>&1 | sed "s/^/$cand: /" | tee -a "$OUT"
done
echo "ALL COLD RUNS DONE" | tee -a "$OUT"
