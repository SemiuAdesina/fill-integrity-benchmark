#!/usr/bin/env bash
#
# Reference solution launcher.
#
# Harbor uploads the whole solution/ directory into the container (by default at
# /solution) and then executes this script by absolute path. The script therefore
# must not assume a working directory: it resolves its own location and reads the
# corrected sources from the fixed/ subtree next to it.
#
# The corrected sources are ordinary .rs files under solution/fixed/, laid out in
# the same relative structure as the crate. Each is copied over its counterpart in
# the candidate tree at /app. Nothing is installed and nothing is fetched.

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
log "target crate: ${APP_DIR}"

# The files this solution replaces, relative to the crate root. Each must exist
# under ${FIXED_DIR} with the same relative path.
FILES=(
    src/types/snapshot.rs
    src/engine/fill_classifier.rs
    src/engine/hedging.rs
    src/engine/cycle.rs
    src/engine/mod.rs
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

if [ ! -f "${APP_DIR}/Cargo.toml" ]; then
    fail "no Rust crate found at ${APP_DIR} (no Cargo.toml)"
fi

cd "${APP_DIR}"

for rel in "${FILES[@]}"; do
    mkdir -p "$(dirname "${APP_DIR}/${rel}")"
    cp "${FIXED_DIR}/${rel}" "${APP_DIR}/${rel}"
    log "applied ${rel}"
done

export CARGO_NET_OFFLINE=true
export CARGO_TERM_COLOR=never

# rustfmt is present in the task image but is not required for correctness, so a
# missing rustfmt must not fail the solution.
if cargo fmt --version >/dev/null 2>&1; then
    cargo fmt || log "cargo fmt reported a problem; continuing"
fi

# Build the library only. The graded suite is compiled by the verifier inside its
# own crate, so building this crate's tests here would prove nothing and would
# fail if the candidate tree carries unrelated test files.
log "building"
cargo build --offline

log "done"
