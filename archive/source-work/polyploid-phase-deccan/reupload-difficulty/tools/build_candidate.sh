#!/usr/bin/env bash
# build_candidate.sh <label> <solver_src_dir>   (solver_src_dir contains solver.rs and optional solver/)
# Builds the public crate natively with that solver installed; binary at lab/cand/<label>/target/release/phase
set -euo pipefail
R=~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty
LABEL="$1"; SRC="$2"; DST="$R/lab/cand/$LABEL"
rm -rf "$DST"; mkdir -p "$R/lab/cand"; cp -R "$R/src-zip/polyploid-phase/environment" "$DST"
rm -rf "$DST/src/solver" "$DST/src/solver.rs" "$DST/target" "$DST/tests"
cp "$SRC/solver.rs" "$DST/src/solver.rs"
[ -d "$SRC/solver" ] && cp -R "$SRC/solver" "$DST/src/solver"
# drop any candidate scratch binaries that would not exist in the verifier's runner crate
rm -rf "$DST/src/bin"; mkdir -p "$DST/src/bin"; cp "$R/src-zip/polyploid-phase/environment/src/bin/phase.rs" "$DST/src/bin/phase.rs"
cd "$DST" && cargo build --release --offline 2> "$DST/build.log" && echo "BUILT $LABEL -> $DST/target/release/phase" || { echo "BUILD FAILED $LABEL"; grep -E "^error" -A6 "$DST/build.log" | head -30; exit 1; }
