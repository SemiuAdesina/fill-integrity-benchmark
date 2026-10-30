#!/usr/bin/env bash
# Local emulation of the Harbor control run. NOT part of the task package.
#   run_control.sh <task_root> <nop|oracle|patched> <label> [patch_dir]
# patched: like nop but overlays runtime.hpp/runtime.cpp from <patch_dir>.
set -uo pipefail

ROOT="$1"; MODE="$2"; LABEL="$3"; PATCH_DIR="${4:-}"
AGENT_TAG="rc-agent:$LABEL"
VER_TAG="rc-verifier:$LABEL"
WORK="$(mktemp -d /tmp/rc-XXXXXX)"
APPDIR="$WORK/app"; LOGDIR="$WORK/logs"
mkdir -p "$APPDIR" "$LOGDIR"

build_flags="${RC_BUILD_FLAGS:-}"
docker build $build_flags -t "$AGENT_TAG" "$ROOT/environment" >"$WORK/agent-build.log" 2>&1 \
  || { echo "AGENT BUILD FAILED"; tail -20 "$WORK/agent-build.log"; exit 1; }
docker build $build_flags -t "$VER_TAG" "$ROOT/tests" >"$WORK/ver-build.log" 2>&1 \
  || { echo "VERIFIER BUILD FAILED"; tail -20 "$WORK/ver-build.log"; exit 1; }

cid=$(docker create "$AGENT_TAG")
docker cp "$cid:/app/." "$APPDIR/" >/dev/null 2>&1
docker rm "$cid" >/dev/null

case "$MODE" in
  oracle)
    docker run --rm --network none -v "$APPDIR:/app" -v "$ROOT/solution:/solution:ro" \
      "$AGENT_TAG" /bin/bash /solution/solve.sh >"$WORK/solve.log" 2>&1 \
      || { echo "SOLVE FAILED"; tail -20 "$WORK/solve.log"; exit 1; }
    ;;
  patched)
    cp "$PATCH_DIR/runtime.hpp" "$APPDIR/include/roundel/runtime.hpp"
    cp "$PATCH_DIR/runtime.cpp" "$APPDIR/src/runtime.cpp"
    ;;
  nop) ;;
esac

docker run --rm --network none -v "$APPDIR:/app" -v "$LOGDIR:/logs/verifier" \
  "$VER_TAG" >"$WORK/verifier.log" 2>&1
rc=$?
reward=$(tr -d '[:space:]' < "$LOGDIR/reward.txt" 2>/dev/null)
summary=$(cat "$LOGDIR/driver-summary.txt" 2>/dev/null | tr '\n' ' ')
echo "MODE=$MODE LABEL=$LABEL REWARD=${reward:-<none>} RC=$rc"
echo "SUMMARY: ${summary:-<none>}"
echo "WORKDIR: $WORK"
