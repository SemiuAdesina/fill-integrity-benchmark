#!/usr/bin/env bash
# Local A6 validation driver for attributable-fill-detection V8.
# Runs the real tests/test.sh (native, via its documented VERIFIER_CARGO escape
# hatch) against the starter, the reference, and a set of realistic partials.
set -uo pipefail

BASE="/Users/macbok/Downloads/benchmark-suite-solvability-repair/fill-v8-work/build/attributable-fill-detection"
V7="/Users/macbok/Downloads/benchmark-suite-solvability-repair/fill-v8-work/src-v7"
WORK="/tmp/fillv8-validate"
export VERIFIER_CARGO="${HOME}/.cargo/bin/cargo"

mkdir -p "${WORK}"

# $1 = variant name. Populates ${WORK}/app from the V8 starter.
stage() {
    rm -rf "${WORK}/app"
    mkdir -p "${WORK}/app"
    cp -R "${BASE}/environment/Cargo.toml" "${BASE}/environment/Cargo.lock" "${BASE}/environment/src" "${WORK}/app/"
}

# Overlay the reference solution onto the staged tree.
overlay_reference() {
    for rel in src/types/snapshot.rs src/engine/fill_classifier.rs \
               src/engine/neutralization.rs src/engine/cycle.rs src/engine/mod.rs; do
        cp "${BASE}/solution/fixed/${rel}" "${WORK}/app/${rel}"
    done
}

# $1 = variant name
judge() {
    local name="$1"
    local logs="${WORK}/logs/${name}"
    rm -rf "${logs}"
    mkdir -p "${logs}"
    local start
    start="$(python3 -c 'import time;print(time.time())')"
    APP_DIR="${WORK}/app" REWARD_DIR="${logs}" \
        bash "${BASE}/tests/test.sh" > "${logs}/stdout.txt" 2>&1
    local elapsed
    elapsed="$(python3 -c "import time;print(f'{time.time()-${start}:.1f}')")"
    local reward
    reward="$(cat "${logs}/reward.txt" 2>/dev/null | tr -d '[:space:]')"
    local passed failed
    passed="$(grep -cE '^test [A-Za-z0-9_:]+ \.\.\. ok$' "${logs}/stdout.txt" 2>/dev/null)"
    failed="$(grep -cE '^test [A-Za-z0-9_:]+ \.\.\. FAILED$' "${logs}/stdout.txt" 2>/dev/null)"
    printf '%-34s reward=%-3s passed=%-3s failed=%-3s %ss\n' \
        "${name}" "${reward:-?}" "${passed}" "${failed}" "${elapsed}"
}
