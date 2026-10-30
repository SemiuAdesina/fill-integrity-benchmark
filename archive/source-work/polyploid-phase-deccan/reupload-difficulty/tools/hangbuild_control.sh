#!/usr/bin/env bash
# Hanging-build control: the candidate's solver compiles forever. The verifier's
# own runner build must hit BUILD_TIMEOUT and fail closed with reward 0.
set -uo pipefail
R=~/benchmark-suite-work/polyploid-phase-deccan/reupload-difficulty
W=$(mktemp -d /tmp/hb-XXXXXX); mkdir -p "$W/app" "$W/logs"
docker build -q -t hb-agent "$R/pkg/environment" >/dev/null 2>&1
cid=$(docker create hb-agent); docker cp "$cid:/app/." "$W/app/" >/dev/null 2>&1; docker rm "$cid" >/dev/null
cp "$R/lab/xc_patch/hangbuild/solver.rs" "$W/app/src/solver.rs"
docker build -q -t hb-ver "$R/pkg/tests" >/dev/null 2>&1
t0=$(date +%s)
docker run --rm --network none -v "$W/app:/app" -v "$W/logs:/logs/verifier" hb-ver >/dev/null 2>&1
rc=$?
echo "hanging build: verifier rc=$rc elapsed=$(( $(date +%s) - t0 ))s reward=$(tr -d '[:space:]' < "$W/logs/reward.txt" 2>/dev/null)"
grep -E "^FATAL" "$W/logs/run.log" 2>/dev/null | head -2
rm -rf "$W"
