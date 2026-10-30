#!/usr/bin/env bash
# build_candidate.sh <label> <src_dir>  (src_dir holds recover.cpp and optional recover/*.cpp) -> lab/cand/<label>/reduction-recover
set -euo pipefail
W=~/benchmark-suite-work/reduction-plan-deccan; E=$W/pkg/environment
LABEL="$1"; SRC="$2"; DST="$W/lab/cand/$LABEL"
rm -rf "$DST"; mkdir -p "$DST/src/recover"
cp "$SRC/recover.cpp" "$DST/src/recover.cpp"
[ -d "$SRC/recover" ] && cp -R "$SRC/recover/." "$DST/src/recover/"
EXTRA=$(ls "$DST"/src/recover/*.cpp 2>/dev/null || true)
if clang++ -std=c++20 -O2 -Wall -Wextra -Wpedantic -I"$E/include" -I"$E/src" -I"$DST/src" "$E/src/json.cpp" "$E/src/model.cpp" "$DST/src/recover.cpp" $EXTRA "$E/src/main.cpp" -o "$DST/reduction-recover" > "$DST/build.log" 2>&1; then echo "BUILT $LABEL"; else echo "BUILD FAILED $LABEL"; grep -E "error" -A4 "$DST/build.log" | head -30; exit 1; fi
