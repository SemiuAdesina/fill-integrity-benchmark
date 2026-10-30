#!/usr/bin/env bash
# build_candidate.sh <label> <src_dir>  (src_dir holds restore.cpp and optional restore/*.cpp)
# Native build with clang++ (no cmake locally): lab/cand/<label>/fp8-restore
set -euo pipefail
W=~/benchmark-suite-work/fp8-reshard-deccan; E=$W/pkg/environment
LABEL="$1"; SRC="$2"; DST="$W/lab/cand/$LABEL"
rm -rf "$DST"; mkdir -p "$DST/src/restore"
cp "$SRC/restore.cpp" "$DST/src/restore.cpp"
[ -d "$SRC/restore" ] && cp -R "$SRC/restore/." "$DST/src/restore/"
EXTRA=$(ls "$DST"/src/restore/*.cpp 2>/dev/null || true)
if clang++ -std=c++20 -O2 -Wall -Wextra -Wpedantic -I"$E/include" -I"$E/src" -I"$DST/src" "$E/src/json.cpp" "$E/src/model.cpp" "$DST/src/restore.cpp" $EXTRA "$E/src/main.cpp" -o "$DST/fp8-restore" > "$DST/build.log" 2>&1; then
  echo "BUILT $LABEL"
else
  echo "BUILD FAILED $LABEL"; grep -E "error" -A4 "$DST/build.log" | head -30; exit 1
fi
