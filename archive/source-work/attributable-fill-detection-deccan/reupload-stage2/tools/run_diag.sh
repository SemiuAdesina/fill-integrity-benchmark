#!/usr/bin/env bash
# Run the redesigned public diagnostics natively against a candidate src tree.
#   run_diag.sh <label> <src_dir> [tests_dir]
set -uo pipefail
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
LABEL="$1"; SRC="$2"; TESTS="${3:-$R/lab/redesign/tests}"
W="$R/lab/diag/$LABEL"; rm -rf "$W"; mkdir -p "$W"
cp -R "$SRC/." "$W/src/"; rm -rf "$W/src/bin"
cp -R "$TESTS" "$W/tests"; cp "$R/pkg/environment/Cargo.toml" "$W/Cargo.toml"; cp "$R/pkg/environment/Cargo.lock" "$W/Cargo.lock"
export CARGO_TARGET_DIR="$R/lab/diag/target-$LABEL"
summary=""
for g in observation_identity direct_progress block_reconciliation neutralization_lifecycle restart_recovery cross_mandate_isolation; do
  out=$(cargo test --offline --manifest-path "$W/Cargo.toml" --test "$g" -- --test-threads=1 2>&1); rc=$?
  if echo "$out" | grep -qE '^error(\[E[0-9]+\])?: (could not compile|expected|unresolved|cannot|mismatched|no |the trait|failed to|use of|unused)|^error\[E'; then summary="$summary $g:BUILD_FAILED"; echo "$out" | grep -m3 -E '^error' | sed "s/^/  [$g] /"; continue; fi
  p=$(echo "$out" | grep -oE '[0-9]+ passed' | tail -1 | awk '{print $1}'); f=$(echo "$out" | grep -oE '[0-9]+ failed' | tail -1 | awk '{print $1}')
  summary="$summary $g:${p:-?}/$(( ${p:-0} + ${f:-0} ))"
  echo "$out" | grep -E '^test .* FAILED|^---- |panicked at|^  left|^ right|^assertion' | sed "s/^/  [$g] /" | head -40
done
echo "$LABEL:$summary"
