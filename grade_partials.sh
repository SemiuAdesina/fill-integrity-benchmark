#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TASK_DIR="${SCRIPT_DIR}/attributable-fill-detection"
VERIFIER_IMAGE="afd-verifier:v4"
PARTIALS_ROOT="/tmp/afd-partials"

if [ ! -d "${PARTIALS_ROOT}" ]; then
    echo "ERROR: no partials at ${PARTIALS_ROOT}. Run make_partials.py first."
    exit 1
fi

grade_partial() {
    local name="$1"
    local src="${PARTIALS_ROOT}/${name}"
    local logdir="/tmp/afd-partial-${name}"

    rm -rf "${logdir}"
    mkdir -p "${logdir}/verifier"

    docker rm -f "afd-partial-${name}" >/dev/null 2>&1 || true
    docker create --name "afd-partial-${name}" \
        -e REWARD_FILE=/logs/verifier/reward.txt \
        -e REPORT_FILE=/logs/verifier/report.json \
        "${VERIFIER_IMAGE}" bash /tests/test.sh >/dev/null

    docker cp "${src}/." "afd-partial-${name}:/app/" >/dev/null
    docker start -a "afd-partial-${name}" >"${logdir}/verifier/stdout.txt" 2>&1 || true

    docker cp "afd-partial-${name}:/logs/verifier/reward.txt" "${logdir}/verifier/reward.txt" >/dev/null 2>&1 \
        || printf '0\n' > "${logdir}/verifier/reward.txt"
    docker cp "afd-partial-${name}:/logs/verifier/report.json" "${logdir}/verifier/report.json" >/dev/null 2>&1 || true
    docker rm -f "afd-partial-${name}" >/dev/null 2>&1 || true

    local reward passed failed
    reward=$(tr -d '[:space:]' < "${logdir}/verifier/reward.txt")
    passed=0; failed=0
    if [ -f "${logdir}/verifier/report.json" ]; then
        passed=$(jq -r '.results.summary.passed // .summary.passed // 0' "${logdir}/verifier/report.json")
        failed=$(jq -r '.results.summary.failed // .summary.failed // 0' "${logdir}/verifier/report.json")
    fi
    printf '  %-35s  reward=%s  passed=%2d  failed=%2d\n' "${name}" "${reward}" "${passed}" "${failed}"

    if [ "${reward}" != "0" ]; then
        echo "    FAIL: expected reward=0"
        return 1
    fi
}

echo "=== PARTIAL-FIX REJECTION BATTERY ==="
echo ""

FAIL_COUNT=0
for variant in P2-no-visibility-rule P3-feed-only-no-open-orders P4-open-orders-only-no-feed \
               P5-retry-under-fresh-client-ref P6-permanent-per-mandate-flag \
               P7-neutralize-full-leg-size P8-ignore-outstanding-neutralizations \
               P9-client-seq-not-persisted P10-ledger-not-persisted; do
    if ! grade_partial "${variant}"; then
        FAIL_COUNT=$((FAIL_COUNT + 1))
    fi
done

echo ""
if [ "${FAIL_COUNT}" -eq 0 ]; then
    echo "PASS: all 9 partial fixes scored 0"
else
    echo "FAIL: ${FAIL_COUNT} partial fixes did not score 0"
    exit 1
fi
