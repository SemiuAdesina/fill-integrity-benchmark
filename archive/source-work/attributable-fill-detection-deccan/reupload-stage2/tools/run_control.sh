#!/usr/bin/env bash
# Local emulation of the Harbor control run for attributable-fill-detection. NOT part of the package.
#   run_control.sh <task_root> <nop|oracle|patched> <label> [candidate_src_dir]
# patched: replaces /app/src with <candidate_src_dir> (a full src tree) before grading.
# Images: afd-agent:<label> / afd-verifier:<label> are built from <task_root> unless AFD_IMAGE_TAG is set,
# in which case the prebuilt afd-agent:$AFD_IMAGE_TAG / afd-verifier:$AFD_IMAGE_TAG are used.
set -uo pipefail
ROOT="$1"; MODE="$2"; LABEL="$3"; PATCH_DIR="${4:-}"
WORK="$(mktemp -d /tmp/afd-XXXXXX)"; APPDIR="$WORK/app"; LOGDIR="$WORK/logs"; mkdir -p "$APPDIR" "$LOGDIR"
if [ -n "${AFD_IMAGE_TAG:-}" ]; then
  AGENT_TAG="afd-agent:$AFD_IMAGE_TAG"; VER_TAG="afd-verifier:$AFD_IMAGE_TAG"
else
  AGENT_TAG="afd-agent:$LABEL"; VER_TAG="afd-verifier:$LABEL"; bf="${RC_BUILD_FLAGS:-}"
  docker build $bf -t "$AGENT_TAG" "$ROOT/environment" >"$WORK/agent-build.log" 2>&1 || { echo "AGENT BUILD FAILED"; tail -20 "$WORK/agent-build.log"; exit 1; }
  docker build $bf -t "$VER_TAG" "$ROOT/tests" >"$WORK/ver-build.log" 2>&1 || { echo "VERIFIER BUILD FAILED"; tail -20 "$WORK/ver-build.log"; exit 1; }
fi
cid=$(docker create "$AGENT_TAG"); docker cp "$cid:/app/." "$APPDIR/" >/dev/null 2>&1; docker rm "$cid" >/dev/null
rm -rf "$APPDIR/target"
case "$MODE" in
  oracle)
    docker run --rm --network none -v "$APPDIR:/app" -v "$ROOT/solution:/solution:ro" "$AGENT_TAG" \
      /bin/bash /solution/solve.sh >"$WORK/solve.log" 2>&1 || { echo "SOLVE FAILED"; tail -20 "$WORK/solve.log"; exit 1; } ;;
  patched)
    rm -rf "$APPDIR/src"; cp -R "$PATCH_DIR" "$APPDIR/src" ;;
  nop) ;;
esac
rm -rf "$APPDIR/target"
t0=$(date +%s)
docker run --rm --network none -v "$APPDIR:/app" -v "$LOGDIR:/logs/verifier" "$VER_TAG" >"$WORK/verifier.log" 2>&1; rc=$?
el=$(( $(date +%s) - t0 ))
reward=$(tr -d '[:space:]' < "$LOGDIR/reward.txt" 2>/dev/null)
verdict=$(awk '/^TOTAL/{print}' "$LOGDIR/verdict.txt" 2>/dev/null)
fails=$(grep '^FAIL ' "$LOGDIR/verdict.txt" 2>/dev/null | awk '{print $2}' | tr '\n' ' ')
echo "MODE=$MODE LABEL=$LABEL REWARD=${reward:-<none>} RC=$rc ELAPSED=${el}s $verdict"
echo "FAILS: ${fails:-<none>}"
echo "RUNLOG: $(grep -E '^FATAL|^timing|^verdict|CONTROL' "$LOGDIR/run.log" 2>/dev/null | tr '\n' ' ')"
echo "WORKDIR: $WORK"
