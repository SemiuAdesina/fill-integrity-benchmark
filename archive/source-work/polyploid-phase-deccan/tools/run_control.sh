#!/usr/bin/env bash
# Local emulation of the Harbor control run for polyploid-phase. NOT part of the package.
#   run_control.sh <task_root> <nop|oracle|patched> <label> [patch_dir]
# patched: overlays <patch_dir>/solver.rs and <patch_dir>/solver/ onto the starter, then runs phase.
set -uo pipefail
ROOT="$1"; MODE="$2"; LABEL="$3"; PATCH_DIR="${4:-}"
AGENT_TAG="pc-agent:$LABEL"; VER_TAG="pc-verifier:$LABEL"
WORK="$(mktemp -d /tmp/pc-XXXXXX)"; APPDIR="$WORK/app"; LOGDIR="$WORK/logs"; mkdir -p "$APPDIR" "$LOGDIR"
bf="${RC_BUILD_FLAGS:-}"
docker build $bf -t "$AGENT_TAG" "$ROOT/environment" >"$WORK/agent-build.log" 2>&1 || { echo "AGENT BUILD FAILED"; tail -20 "$WORK/agent-build.log"; exit 1; }
docker build $bf -t "$VER_TAG" "$ROOT/tests" >"$WORK/ver-build.log" 2>&1 || { echo "VERIFIER BUILD FAILED"; tail -20 "$WORK/ver-build.log"; exit 1; }
cid=$(docker create "$AGENT_TAG"); docker cp "$cid:/app/." "$APPDIR/" >/dev/null 2>&1; docker rm "$cid" >/dev/null
case "$MODE" in
  oracle)
    docker run --rm --network none -v "$APPDIR:/app" -v "$ROOT/solution:/solution:ro" "$AGENT_TAG" \
      /bin/bash /solution/solve.sh >"$WORK/solve.log" 2>&1 || { echo "SOLVE FAILED"; tail -20 "$WORK/solve.log"; exit 1; } ;;
  patched)
    rm -rf "$APPDIR/src/solver" "$APPDIR/src/solver.rs"
    cp "$PATCH_DIR/solver.rs" "$APPDIR/src/solver.rs"
    [ -d "$PATCH_DIR/solver" ] && cp -R "$PATCH_DIR/solver" "$APPDIR/src/solver"
    docker run --rm --network none -v "$APPDIR:/app" "$AGENT_TAG" \
      /bin/bash -c 'cd /app && cargo build --offline --release >/tmp/b.log 2>&1 && ./target/release/phase' >"$WORK/patch.log" 2>&1 || echo "  (patched build/run failed: $(tail -3 "$WORK/patch.log" | tr '\n' ' '))" ;;
  nop) ;;
esac
docker run --rm --network none -v "$APPDIR:/app" -v "$LOGDIR:/logs/verifier" "$VER_TAG" >"$WORK/verifier.log" 2>&1; rc=$?
reward=$(tr -d '[:space:]' < "$LOGDIR/reward.txt" 2>/dev/null)
verdict=$(grep -E '^verdict|^PASS|^FAIL|^CONTROL|^FATAL' "$LOGDIR/run.log" 2>/dev/null | tr '\n' ' ')
echo "MODE=$MODE LABEL=$LABEL REWARD=${reward:-<none>} RC=$rc"
echo "RUNLOG: $verdict"
echo "WORKDIR: $WORK"
