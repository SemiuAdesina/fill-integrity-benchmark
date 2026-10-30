#!/usr/bin/env bash
#
# Grading entry point.
#
# Threat model
# ------------
# Everything under /app was writable by the candidate, including the crate
# manifest, any .cargo/config.toml, build scripts, and (in a shared-image
# deployment) the cargo binary itself. Running `cargo test` inside /app would let
# a candidate fake a pass without solving anything: a [target.*.runner] shim, a
# RUSTC_WRAPPER, a build.rs that writes the reward file, or a `cargo` earlier on
# PATH all produce "all tests passed" from a broken engine.
#
# So nothing from /app is trusted except the library sources themselves. This
# script assembles a fresh crate in a verifier-owned directory:
#
#   - Cargo.toml and Cargo.lock are the verifier's copies, baked into this image
#   - src/ is copied from the candidate (this is what is being graded)
#   - tests/ are the verifier's copies, baked into this image
#   - build.rs and .cargo/ from the candidate are dropped on the floor
#   - cargo is invoked by absolute path, with a scrubbed environment
#
# The reward file is written by this script alone, and starts at 0.

set -uo pipefail

APP_DIR="${APP_DIR:-/app}"
TESTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REWARD_DIR="${REWARD_DIR:-/logs/verifier}"
REWARD_FILE="${REWARD_DIR}/reward.txt"
CTRF_FILE="${CTRF_FILE:-${REWARD_DIR}/report.json}"

mkdir -p "${REWARD_DIR}"
printf '0\n' > "${REWARD_FILE}"

# ---------------------------------------------------------------------------
# Environment scrubbing
#
# Each of these can redirect compilation or test execution to candidate-supplied
# code. They are cleared unconditionally rather than inspected, because an
# allowlist of "safe" values is easier to get wrong than a blanket unset.
# ---------------------------------------------------------------------------
unset RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset CARGO_BUILD_RUSTC CARGO_TARGET_DIR CARGO_BUILD_TARGET_DIR
unset RUSTC RUSTDOC RUSTFLAGS RUSTDOCFLAGS CARGO_ENCODED_RUSTFLAGS
unset CARGO_BUILD_RUSTFLAGS LD_PRELOAD LD_LIBRARY_PATH
# CARGO_HOME is inherited too, and pointing it at a candidate-writable directory
# would redirect toolchain lookup. It is re-derived below from trusted prefixes.
unset CARGO_HOME
for var in $(env | grep -oE '^CARGO_TARGET_[A-Z0-9_]+_RUNNER' || true); do
    unset "${var}"
done
for var in $(env | grep -oE '^CARGO_TARGET_[A-Z0-9_]+_LINKER' || true); do
    unset "${var}"
done

# A verifier-owned HOME, so ~/.cargo/config.toml is ours and not the candidate's.
# Where the toolchain is managed by rustup, RUSTUP_HOME lives under the original
# HOME and must be preserved explicitly or the toolchain stops resolving once HOME
# moves. The official rust image ships a real cargo binary and no rustup home, in
# which case there is nothing to carry over.
if [ -z "${RUSTUP_HOME:-}" ] && [ -d "${HOME:-}/.rustup" ]; then
    export RUSTUP_HOME="${HOME}/.rustup"
fi

VERIFY_ROOT="$(mktemp -d /tmp/verify.XXXXXXXX)"
export HOME="${VERIFY_ROOT}/home"
mkdir -p "${HOME}"

# ---------------------------------------------------------------------------
# Toolchain resolution
#
# cargo is located by scanning a fixed list of trusted absolute prefixes and is
# invoked by absolute path. PATH is never consulted, because PATH is inherited
# from the candidate's environment: a `cargo` planted in a directory earlier on it
# would otherwise be picked up and could print a passing suite without compiling
# anything. Only directories that are root-owned in the task images appear here.
# ---------------------------------------------------------------------------
CARGO_BIN=""
for _candidate_home in \
    /usr/local/cargo \
    /usr/local \
    /usr
do
    if [ -x "${_candidate_home}/bin/cargo" ]; then
        CARGO_HOME="${_candidate_home}"
        CARGO_BIN="${_candidate_home}/bin/cargo"
        break
    fi
done

# Escape hatch for running this script outside the task image (local validation on
# a rustup-managed host). Read from VERIFIER_CARGO, which the grading images never
# set, so it cannot be used to redirect a real trial: inside the image the loop
# above has already found /usr/local/cargo/bin/cargo and this is skipped.
if [ -z "${CARGO_BIN}" ] && [ -n "${VERIFIER_CARGO:-}" ] && [ -x "${VERIFIER_CARGO}" ]; then
    CARGO_BIN="${VERIFIER_CARGO}"
    CARGO_HOME="$(cd "$(dirname "${CARGO_BIN}")/.." && pwd)"
fi

export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
export CARGO_NET_OFFLINE=true
export CARGO_TERM_COLOR=never

# Narrow PATH to the resolved toolchain plus system directories, so any child
# process cargo spawns (rustc, the linker) also resolves outside candidate control.
export PATH="${CARGO_HOME}/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

RAW_LOG="${VERIFY_ROOT}/test-output.log"
: > "${RAW_LOG}"

# ---------------------------------------------------------------------------
# CTRF reporting
#
# Parsed from real `cargo test` stdout. If the suite never ran, this emits an
# empty report and the reward stays 0.
# ---------------------------------------------------------------------------
emit_ctrf() {
    local passed=0
    local failed=0
    local total=0
    local entries=""
    local line
    local name
    local status

    while IFS= read -r line; do
        name="${line#test }"
        name="${name%% ...*}"
        case "${line}" in
            *"... ok") status="passed" ;;
            *"... FAILED") status="failed" ;;
            *) continue ;;
        esac
        if [ "${status}" = "passed" ]; then
            passed=$((passed + 1))
        else
            failed=$((failed + 1))
        fi
        total=$((total + 1))
        if [ -n "${entries}" ]; then
            entries="${entries},"
        fi
        entries="${entries}
      {\"name\": \"${name}\", \"status\": \"${status}\", \"duration\": 0}"
    done < <(grep -E '^test [A-Za-z0-9_:]+ \.\.\. (ok|FAILED)$' "${RAW_LOG}" 2>/dev/null)

    cat > "${CTRF_FILE}" <<CTRF
{
  "reportFormat": "CTRF",
  "specVersion": "0.0.0",
  "results": {
    "tool": { "name": "cargo-test" },
    "summary": {
      "tests": ${total},
      "passed": ${passed},
      "failed": ${failed},
      "pending": 0,
      "skipped": 0,
      "other": 0,
      "start": 0,
      "stop": 0
    },
    "tests": [${entries}
    ]
  }
}
CTRF
}

fail_out() {
    echo "verifier: $1" >&2
    emit_ctrf
    exit 1
}

# ---------------------------------------------------------------------------
# Assemble the clean crate
# ---------------------------------------------------------------------------
if [ -z "${CARGO_BIN}" ] || [ ! -x "${CARGO_BIN}" ]; then
    fail_out "no cargo toolchain found"
fi

if [ ! -d "${APP_DIR}/src" ]; then
    fail_out "no source tree at ${APP_DIR}/src"
fi

for required in "${TESTS_DIR}/Cargo.toml" "${TESTS_DIR}/Cargo.lock"; do
    [ -f "${required}" ] || fail_out "missing verifier manifest ${required}"
done

CRATE_DIR="${VERIFY_ROOT}/crate"
mkdir -p "${CRATE_DIR}"

# Candidate sources. Only .rs files are taken, so a stray .cargo directory,
# build.rs, or config file inside src/ cannot ride along.
mkdir -p "${CRATE_DIR}/src"
(cd "${APP_DIR}/src" && find . -type f -name '*.rs' -print0 \
    | while IFS= read -r -d '' f; do
        mkdir -p "${CRATE_DIR}/src/$(dirname "${f}")"
        cp "${APP_DIR}/src/${f}" "${CRATE_DIR}/src/${f}"
    done)

if [ -z "$(find "${CRATE_DIR}/src" -name '*.rs' -print -quit)" ]; then
    fail_out "candidate source tree contains no .rs files"
fi

# src/bin/ holds the candidate-facing reproducer, which is a debugging aid and is
# not graded. Dropping it means a broken or hostile reproducer cannot fail or
# hijack the build.
rm -rf "${CRATE_DIR}/src/bin"

# Verifier-owned manifest, lockfile and suite.
cp "${TESTS_DIR}/Cargo.toml" "${CRATE_DIR}/Cargo.toml"
cp "${TESTS_DIR}/Cargo.lock" "${CRATE_DIR}/Cargo.lock"

mkdir -p "${CRATE_DIR}/tests/common"
cp "${TESTS_DIR}/common/mod.rs" "${CRATE_DIR}/tests/common/mod.rs"
cp "${TESTS_DIR}/graded.rs" "${CRATE_DIR}/tests/graded.rs"
cp "${TESTS_DIR}/frozen.rs" "${CRATE_DIR}/tests/frozen.rs"

# An empty, verifier-owned cargo config shadows any inherited runner or wrapper
# settings for this build.
mkdir -p "${CRATE_DIR}/.cargo"
cat > "${CRATE_DIR}/.cargo/config.toml" <<'CARGOCFG'
[build]
rustc-wrapper = ""
CARGOCFG

cd "${CRATE_DIR}" || fail_out "cannot enter ${CRATE_DIR}"

# ---------------------------------------------------------------------------
# Run the suite
#
# --offline so a candidate cannot introduce a dependency, and --test-threads=1
# because the harness uses a shared virtual clock.
# ---------------------------------------------------------------------------
"${CARGO_BIN}" test --offline --test graded --test frozen \
    --manifest-path "${CRATE_DIR}/Cargo.toml" \
    -- --test-threads=1 2>&1 | tee "${RAW_LOG}"
RESULT="${PIPESTATUS[0]}"

emit_ctrf

# ---------------------------------------------------------------------------
# Reward decision
#
# Fails closed. cargo exiting 0 is necessary but not sufficient: the run must also
# report the exact number of passing tests the verifier shipped, counted from its
# own copies of graded.rs and frozen.rs rather than from a constant. A process that
# prints a plausible summary without compiling the suite cannot satisfy this, and
# neither can a run that silently compiled fewer test targets than expected.
# ---------------------------------------------------------------------------
EXPECTED_GRADED="$(grep -cE '^\s*#\[test\]' "${TESTS_DIR}/graded.rs" 2>/dev/null)" || EXPECTED_GRADED=0
EXPECTED_FROZEN="$(grep -cE '^\s*#\[test\]' "${TESTS_DIR}/frozen.rs" 2>/dev/null)" || EXPECTED_FROZEN=0
EXPECTED_TOTAL=$((EXPECTED_GRADED + EXPECTED_FROZEN))

OBSERVED_PASSED="$(grep -cE '^test [A-Za-z0-9_:]+ \.\.\. ok$' "${RAW_LOG}" 2>/dev/null)" || OBSERVED_PASSED=0
OBSERVED_FAILED="$(grep -cE '^test [A-Za-z0-9_:]+ \.\.\. FAILED$' "${RAW_LOG}" 2>/dev/null)" || OBSERVED_FAILED=0

printf 'verifier: expected %s passing tests (graded %s + frozen %s)\n' \
    "${EXPECTED_TOTAL}" "${EXPECTED_GRADED}" "${EXPECTED_FROZEN}"
printf 'verifier: observed %s passed, %s failed, cargo exit %s\n' \
    "${OBSERVED_PASSED}" "${OBSERVED_FAILED}" "${RESULT}"

if [ "${RESULT}" -eq 0 ] \
    && [ "${EXPECTED_TOTAL}" -gt 0 ] \
    && [ "${OBSERVED_FAILED}" -eq 0 ] \
    && [ "${OBSERVED_PASSED}" -eq "${EXPECTED_TOTAL}" ]; then
    printf '1\n' > "${REWARD_FILE}"
    printf 'verifier: reward 1\n'
else
    printf '0\n' > "${REWARD_FILE}"
    printf 'verifier: reward 0\n'
fi

exit "${RESULT}"
