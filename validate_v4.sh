#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TASK_DIR="${SCRIPT_DIR}/attributable-fill-detection"
AGENT_IMAGE="afd-agent:v4"
VERIFIER_IMAGE="afd-verifier:v4"

# run_mode <label> <agent-command>
#   Materialises /app from the agent image, runs <agent-command> inside the agent
#   container, extracts the resulting /app, then runs the separate verifier
#   container against it. Mirrors Harbor's SEPARATE verifier flow: the agent
#   container never sees tests/ or solution/ unless explicitly mounted.
run_mode() {
    local label="$1"
    local agent_cmd="$2"
    local mount_solution="${3:-no}"
    local logdir="/tmp/afd-${label}-logs"
    local appdir="/tmp/afd-${label}-app"

    rm -rf "${logdir}" "${appdir}"
    mkdir -p "${logdir}/agent" "${logdir}/verifier"

    local create_args=(--name "afd-${label}-agent" -w /app)
    if [ "${mount_solution}" = "yes" ]; then
        create_args+=(-v "${TASK_DIR}/solution:/solution:ro")
    fi

    docker rm -f "afd-${label}-agent" >/dev/null 2>&1 || true
    docker create "${create_args[@]}" "${AGENT_IMAGE}" bash -c "${agent_cmd}" >/dev/null
    docker start -a "afd-${label}-agent" >"${logdir}/agent/stdout.txt" 2>&1 || true
    tail -3 "${logdir}/agent/stdout.txt" | sed 's/^/    /'

    docker cp "afd-${label}-agent:/app" "${appdir}" >/dev/null
    docker rm -f "afd-${label}-agent" >/dev/null 2>&1 || true

    docker rm -f "afd-${label}-verifier" >/dev/null 2>&1 || true
    docker create --name "afd-${label}-verifier" \
        -e REWARD_FILE=/logs/verifier/reward.txt \
        -e REPORT_FILE=/logs/verifier/report.json \
        "${VERIFIER_IMAGE}" bash /tests/test.sh >/dev/null
    docker cp "${appdir}/." "afd-${label}-verifier:/app/" >/dev/null
    docker start -a "afd-${label}-verifier" >"${logdir}/verifier/stdout.txt" 2>&1 || true

    docker cp "afd-${label}-verifier:/logs/verifier/reward.txt" "${logdir}/verifier/reward.txt" >/dev/null 2>&1 \
        || printf '0\n' > "${logdir}/verifier/reward.txt"
    docker cp "afd-${label}-verifier:/logs/verifier/report.json" "${logdir}/verifier/report.json" >/dev/null 2>&1 || true
    docker rm -f "afd-${label}-verifier" >/dev/null 2>&1 || true

    local reward passed failed
    reward=$(tr -d '[:space:]' < "${logdir}/verifier/reward.txt")
    passed=0; failed=0
    if [ -f "${logdir}/verifier/report.json" ]; then
        passed=$(jq -r '.results.summary.passed // .summary.passed // 0' "${logdir}/verifier/report.json")
        failed=$(jq -r '.results.summary.failed // .summary.failed // 0' "${logdir}/verifier/report.json")
    fi
    printf '    reward=%s  passed=%s  failed=%s\n' "${reward}" "${passed}" "${failed}"
    printf '%s' "${reward}" > "/tmp/afd-${label}-reward"
}

echo "=== V4 VALIDATION SUITE ==="
echo ""
echo "[1/2] ORACLE (expect reward=1, 30 passed / 0 failed)"
run_mode oracle '/solution/solve.sh' yes
echo ""
echo "[2/2] NOP (expect reward=0, 13 passed / 17 failed)"
run_mode nop 'cargo build --offline'
echo ""
echo "=== SUMMARY ==="
echo "ORACLE reward: $(cat /tmp/afd-oracle-reward)"
echo "NOP    reward: $(cat /tmp/afd-nop-reward)"
