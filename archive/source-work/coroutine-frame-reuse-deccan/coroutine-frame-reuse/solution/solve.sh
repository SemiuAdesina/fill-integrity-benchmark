#!/usr/bin/env bash
#
# Reference solution launcher.
#
# Harbor uploads the whole solution/ directory into the container (by default at
# /solution) and then runs this script by absolute path. It therefore must not
# assume a working directory: it resolves its own location and reads the
# corrected sources from the fixed/ subtree beside it.
#
# The corrected sources are ordinary C++ files under solution/fixed/, laid out in
# the same relative structure as the project. Each is copied over its counterpart
# at /app. Nothing is installed and nothing is fetched.

set -euo pipefail

APP_DIR="${APP_DIR:-/app}"
SOLUTION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXED_DIR="${SOLUTION_DIR}/fixed"

log() {
    printf '[solve] %s\n' "$*"
}

fail() {
    printf '[solve] ERROR: %s\n' "$*" >&2
    exit 1
}

log "solution dir: ${SOLUTION_DIR}"
log "target project: ${APP_DIR}"

# The files this solution replaces, relative to the project root. Each must exist
# under ${FIXED_DIR} at the same relative path.
FILES=(
    include/roundel/runtime.hpp
    src/runtime.cpp
)

# Fail loudly and specifically rather than letting a bare `cp` fail with an
# opaque message. A missing source file means the upload did not land where this
# script expects, which is the single most useful thing to know from a log.
if [ ! -d "${FIXED_DIR}" ]; then
    log "contents of ${SOLUTION_DIR}:"
    ls -la "${SOLUTION_DIR}" >&2 || true
    fail "no fixed/ directory at ${FIXED_DIR}"
fi

missing=()
for rel in "${FILES[@]}"; do
    if [ ! -f "${FIXED_DIR}/${rel}" ]; then
        missing+=("${rel}")
    fi
done

if [ "${#missing[@]}" -ne 0 ]; then
    log "tree under ${FIXED_DIR}:"
    find "${FIXED_DIR}" -type f >&2 || true
    fail "missing source files under ${FIXED_DIR}: ${missing[*]}"
fi

if [ ! -f "${APP_DIR}/CMakeLists.txt" ]; then
    fail "no project found at ${APP_DIR} (no CMakeLists.txt)"
fi

for rel in "${FILES[@]}"; do
    mkdir -p "$(dirname "${APP_DIR}/${rel}")"
    cp "${FIXED_DIR}/${rel}" "${APP_DIR}/${rel}"
    log "applied ${rel}"
done

# clang-format is present in the task image but is not required for correctness,
# so a missing or unhappy clang-format must not fail the solution.
if command -v clang-format >/dev/null 2>&1; then
    for rel in "${FILES[@]}"; do
        clang-format -i "${APP_DIR}/${rel}" 2>/dev/null \
            || log "clang-format skipped ${rel}"
    done
fi

# Build to prove the tree still compiles. The graded suite is compiled by the
# verifier in its own directory, so nothing here affects grading.
log "building"
BUILD_DIR="${APP_DIR}/build"
cmake -S "${APP_DIR}" -B "${BUILD_DIR}" -DCMAKE_BUILD_TYPE=Debug >/dev/null
cmake --build "${BUILD_DIR}" -j "$(nproc 2>/dev/null || echo 2)"

log "done"
