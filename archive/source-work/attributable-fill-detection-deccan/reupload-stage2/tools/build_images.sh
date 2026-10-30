#!/usr/bin/env bash
# Build the agent and verifier images from a task root, plus a replay image (agent + python3).
#   build_images.sh <task_root> <tag>
set -uo pipefail
ROOT="$1"; TAG="$2"
docker build -t "afd-agent:$TAG" "$ROOT/environment" > /tmp/afd-agent-build-$TAG.log 2>&1 || { echo "AGENT BUILD FAILED"; tail -20 /tmp/afd-agent-build-$TAG.log; exit 1; }
docker build -t "afd-verifier:$TAG" "$ROOT/tests" > /tmp/afd-verifier-build-$TAG.log 2>&1 || { echo "VERIFIER BUILD FAILED"; tail -20 /tmp/afd-verifier-build-$TAG.log; exit 1; }
printf 'FROM afd-agent:%s\nRUN apt-get update >/dev/null 2>&1 && apt-get install -y --no-install-recommends python3 jq >/dev/null 2>&1 || true\n' "$TAG" > /tmp/afd-replay-$TAG.Dockerfile
docker build -t "afd-replay:$TAG" -f /tmp/afd-replay-$TAG.Dockerfile /tmp > /tmp/afd-replay-build-$TAG.log 2>&1 || { echo "REPLAY BUILD FAILED"; tail -20 /tmp/afd-replay-build-$TAG.log; exit 1; }
echo "IMAGES BUILT afd-agent:$TAG afd-verifier:$TAG afd-replay:$TAG"
docker run --rm afd-replay:$TAG python3 --version
