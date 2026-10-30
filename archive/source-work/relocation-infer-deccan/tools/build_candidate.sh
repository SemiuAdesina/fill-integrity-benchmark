#!/usr/bin/env bash
# build_candidate.sh <label> <src_dir>   (src_dir holds recover.rs and optional recover/)
# Builds the public crate natively with that recover module installed: lab/cand/<label>/target/release/reloc-recover
set -euo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan
LABEL="$1"; SRC="$2"; DST="$W/lab/cand/$LABEL"
rm -rf "$DST"; mkdir -p "$W/lab/cand"; cp -R "$W/pkg/environment" "$DST"
rm -rf "$DST/src/recover" "$DST/src/recover.rs" "$DST/target" "$DST/corpus"
cp "$SRC/recover.rs" "$DST/src/recover.rs"
[ -d "$SRC/recover" ] && cp -R "$SRC/recover" "$DST/src/recover"
cd "$DST" && cargo build --release --offline > "$DST/build.log" 2>&1 && echo "BUILT $LABEL" || { echo "BUILD FAILED $LABEL"; grep -E "^(error|warning)" -A8 "$DST/build.log" | head -60; exit 1; }
