#!/usr/bin/env bash
# Native lab runner: build the lab graded runner against a candidate src tree and
# record the raw observation for each named case.
#   probe_cases.sh <label> <src_dir> <case...>
set -uo pipefail
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
LABEL="$1"; SRC="$2"; shift 2
W="$R/lab/probe/$LABEL"; rm -rf "$W"; mkdir -p "$W/runner/tests/common" "$W/runner/src/bin" "$W/results"
cp -R "$SRC/." "$W/runner/src/"; rm -rf "$W/runner/src/bin"; mkdir -p "$W/runner/src/bin"
cp "$R/lab/tests/Cargo.toml" "$W/runner/Cargo.toml"; cp "$R/lab/tests/Cargo.lock" "$W/runner/Cargo.lock"
cp "$R/lab/tests/graded.rs" "$W/runner/tests/graded.rs"; cp "$R/lab/tests/common/mod.rs" "$W/runner/tests/common/mod.rs"
cp "$R/pkg/tests/frozen_probe.rs" "$W/runner/src/bin/frozen_probe.rs"
export CARGO_TARGET_DIR="$R/lab/probe/target-$LABEL"
if ! cargo test --offline --manifest-path "$W/runner/Cargo.toml" --test graded --no-run > "$W/build.log" 2>&1; then echo "$LABEL: BUILD FAILED"; grep -m5 -E '^error' "$W/build.log"; exit 1; fi
BIN=$(find "$CARGO_TARGET_DIR/debug/deps" -maxdepth 1 -type f -name 'graded-*' -perm -111 | head -1)
for c in "$@"; do FILL_OBSERVATION_FILE="$W/results/$c" "$BIN" --exact "$c" --test-threads=1 > "$W/results/$c.out" 2>&1 || true; done
echo "$LABEL: probed $# cases"
