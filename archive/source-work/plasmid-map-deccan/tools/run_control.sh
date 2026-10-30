#!/usr/bin/env bash
# Local emulation of the Harbor control run for plasmid-map. NOT part of the package.
#   run_control.sh <task_root> <nop|oracle|patched|overlay> <label> [patch_dir]
# patched: patch_dir holds solver.rs and optionally solver/ (installed over /app/src, built in the agent image)
set -uo pipefail
ROOT="$1"; MODE="$2"; LABEL="$3"; PATCH_DIR="${4:-}"
AGENT_TAG="pm-agent:${RC_AGENT_TAG:-$LABEL}"; VER_TAG="pm-verifier:${RC_VER_TAG:-$LABEL}"
WORK="$(mktemp -d /tmp/rcp-XXXXXX)"; APPDIR="$WORK/app"; LOGDIR="$WORK/logs"; mkdir -p "$APPDIR" "$LOGDIR"
if [ -z "${RC_SKIP_BUILD:-}" ]; then
  docker build -q -t "$AGENT_TAG" "$ROOT/environment" >"$WORK/agent-build.log" 2>&1 || { echo "AGENT BUILD FAILED"; tail -20 "$WORK/agent-build.log"; exit 1; }
  docker build -q -t "$VER_TAG" "$ROOT/tests" >"$WORK/ver-build.log" 2>&1 || { echo "VERIFIER BUILD FAILED"; tail -20 "$WORK/ver-build.log"; exit 1; }
fi
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
      /bin/bash -c 'cd /app && timeout -k 5 600 cargo build --offline --release >/tmp/b.log 2>&1 && timeout -k 5 120 ./target/release/plasmid-map' >"$WORK/patch.log" 2>&1 || echo "  (patched build/run failed: $(tail -3 "$WORK/patch.log" | tr '\n' ' '))" ;;
  overlay)
    cp -R "$PATCH_DIR/." "$APPDIR/" ;;
  nop) ;;
esac
if [ -n "${RC_PRE_REWARD:-}" ]; then printf '%s\n' "$RC_PRE_REWARD" > "$LOGDIR/reward.txt"; fi
if [ -n "${RC_TEST_SH:-}" ]; then
  VER_ALT="$(mktemp -d /tmp/rcpv-XXXXXX)"; cp -R "$ROOT/tests/." "$VER_ALT/"; cp "$RC_TEST_SH" "$VER_ALT/test.sh"
  docker build -q -t "$VER_TAG-alt" "$VER_ALT" >/dev/null 2>&1 && VER_TAG="$VER_TAG-alt"; rm -rf "$VER_ALT"
fi
t0=$(date +%s)
docker run --rm --network none -v "$APPDIR:/app" -v "$LOGDIR:/logs/verifier" "$VER_TAG" >"$WORK/verifier.log" 2>&1; rc=$?
reward=$(tr -d '[:space:]' < "$LOGDIR/reward.txt" 2>/dev/null)
verdict=$(grep -E '^verdict|^FATAL|^CONTROL' "$LOGDIR/run.log" 2>/dev/null | tr '\n' ' ')
fails=$(grep -E '^FAIL ' "$LOGDIR/run.log" 2>/dev/null | cut -d' ' -f2 | tr '\n' ',')
echo "MODE=$MODE LABEL=$LABEL REWARD=${reward:-<none>} RC=$rc ELAPSED=$(( $(date +%s) - t0 ))s"
echo "RUNLOG: $verdict FAILS: ${fails:-none}"
echo "WORKDIR: $WORK"
