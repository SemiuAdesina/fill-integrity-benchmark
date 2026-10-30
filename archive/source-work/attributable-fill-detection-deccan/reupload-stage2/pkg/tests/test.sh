#!/usr/bin/env bash
# Class C verifier entry point.
#
# Candidate source is built into observation executables only. Those executables
# run as an unprivileged account and emit bounded raw functional artifacts. A
# different verifier-owned process, judge.sh, owns private expectations and the
# final comparison. Candidate stdout, stderr, exit status and test status lines
# are diagnostic only and never grant reward.
set -uo pipefail

LOCAL_RUSTUP_HOME="${RUSTUP_HOME:-${HOME:-/nonexistent}/.rustup}"
export PATH=/usr/local/cargo/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
unset RUSTC RUSTDOC RUSTFLAGS RUSTDOCFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
unset CARGO_BUILD_RUSTC_WRAPPER CARGO_BUILD_RUSTC CARGO_BUILD_RUSTFLAGS
unset CARGO_TARGET_DIR CARGO_BUILD_TARGET CARGO_BUILD_TARGET_DIR
unset CARGO_ENCODED_RUSTFLAGS CARGO_MANIFEST_DIR CARGO_MANIFEST_PATH
unset RUSTUP_TOOLCHAIN RUSTUP_DIST_SERVER RUSTUP_UPDATE_ROOT
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY BASH_ENV ENV CDPATH
for variable in $(/usr/bin/env | /usr/bin/grep -oE '^CARGO_TARGET_[A-Z0-9_]+_(RUNNER|LINKER)' || true); do
    unset "$variable"
done

APP_DIR="${APP_DIR:-/app}"
TESTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOGS="${REWARD_DIR:-/logs/verifier}"
REWARD="$LOGS/reward.txt"
REPORT="${CTRF_FILE:-$LOGS/report.json}"
mkdir -p "$LOGS"
# The graded account must not be able to reach the reward or the report: the
# directory is closed to everyone but the verifier before any candidate code runs.
chmod 700 "$LOGS" 2>/dev/null || true
rm -rf "$REWARD" "$REPORT"
printf '0\n' > "$REWARD"
: > "$LOGS/run.log"
log() { printf '%s\n' "$*" >> "$LOGS/run.log"; }

# Budgets, in seconds. The candidate-linked build gets BUILD_TIMEOUT. Each case
# gets CASE_TIMEOUT, and the whole candidate phase must end within PHASE_BUDGET of
# verifier start. The first case to exhaust its budget fails the run closed: full
# credit is already impossible, so no further candidate code is run and the
# verifier finishes normally with reward 0, well inside its own envelope.
BUILD_TIMEOUT=300
CASE_TIMEOUT=60
PHASE_BUDGET=600
VERIFIER_STARTED="$(date +%s)"
TIMEOUT_BIN=/usr/bin/timeout

fail() {
    log "FATAL $1"
    printf '{"reportFormat":"CTRF","results":{"tool":{"name":"fill-verifier-owned"},"summary":{"tests":0,"passed":0,"failed":0},"tests":[]}}\n' > "$REPORT"
    exit 2
}

[ -d "$APP_DIR/src" ] || fail 'candidate source tree missing'
[ -f "$TESTS_DIR/Cargo.toml" ] || fail 'verifier manifest missing'
[ -f "$TESTS_DIR/Cargo.lock" ] || fail 'verifier lockfile missing'
[ -f "$TESTS_DIR/graded.rs" ] || fail 'graded scenarios missing'
[ -f "$TESTS_DIR/frozen_probe.rs" ] || fail 'frozen probe missing'
[ -f "$TESTS_DIR/cases.txt" ] || fail 'case manifest missing'
[ -x "$TESTS_DIR/judge.sh" ] || fail 'judge missing'
[ -d "$TESTS_DIR/expected" ] || fail 'private expectations missing'

# The candidate source contract allows Rust modules under src, but never
# symlinks, manifests, build scripts, cargo configuration or prebuilt objects.
if find "$APP_DIR" -type l -print -quit | /usr/bin/grep -q .; then
    fail 'symlink present under candidate app'
fi

CARGO_BIN=/usr/local/cargo/bin/cargo
RUSTC_BIN=/usr/local/cargo/bin/rustc
RUNUSER=/usr/sbin/runuser
GRADED_USER=nobody

# Native developer validation uses an explicitly supplied trusted cargo path;
# the final Linux image always takes the fixed paths above.
if [ ! -x "$CARGO_BIN" ] && [ -n "${VERIFIER_CARGO:-}" ] && [ -x "$VERIFIER_CARGO" ]; then
    CARGO_BIN="$VERIFIER_CARGO"
    RUSTC_BIN="$(dirname "$VERIFIER_CARGO")/rustc"
fi
[ -x "$CARGO_BIN" ] && [ -x "$RUSTC_BIN" ] || fail 'trusted Rust toolchain missing'

VERIFY="$(mktemp -d /tmp/fill-verify.XXXXXXXX)" || fail 'cannot create private workspace'
SANDBOX="$(mktemp -d /tmp/fill-runner.XXXXXXXX)" || fail 'cannot create runner workspace'
trap 'rm -rf "$VERIFY" "$SANDBOX"' EXIT
chmod 700 "$VERIFY"
chmod 755 "$SANDBOX"

export HOME="$VERIFY/home"
export CARGO_HOME="$(cd "$(dirname "$CARGO_BIN")/.." && pwd)"
export CARGO_NET_OFFLINE=true
export CARGO_TERM_COLOR=never
export RUSTC="$RUSTC_BIN"
mkdir -p "$HOME"

# Assemble the candidate-linked runner from an allowlisted source shape and
# verifier-owned manifest/scenarios. Only regular Rust source files are copied.
RUNNER="$SANDBOX/runner"
mkdir -p "$RUNNER/src" "$RUNNER/src/bin" "$RUNNER/tests/common" "$RUNNER/.cargo"
(cd "$APP_DIR/src" && find . -type f -name '*.rs' -print0 | while IFS= read -r -d '' file; do
    case "$file" in ./bin/*) continue ;; esac
    mkdir -p "$RUNNER/src/$(dirname "$file")"
    cp "$APP_DIR/src/$file" "$RUNNER/src/$file"
done)
[ -f "$RUNNER/src/lib.rs" ] || fail 'candidate library entry point missing'
cp "$TESTS_DIR/Cargo.toml" "$RUNNER/Cargo.toml"
cp "$TESTS_DIR/Cargo.lock" "$RUNNER/Cargo.lock"
cp "$TESTS_DIR/graded.rs" "$RUNNER/tests/graded.rs"
cp "$TESTS_DIR/common/mod.rs" "$RUNNER/tests/common/mod.rs"
cp "$TESTS_DIR/frozen_probe.rs" "$RUNNER/src/bin/frozen_probe.rs"
printf '[build]\nrustc-wrapper = ""\n' > "$RUNNER/.cargo/config.toml"

# In the Linux verifier image the graded account must not be able to read the
# private expectations or judge. Native developer validation has no account
# boundary and must not mutate the checked-in source tree's file modes.
# The graded account is mandatory: candidate-linked code is never built or run
# as root. A verifier image without runuser or the nobody account fails closed.
[ -x "$RUNUSER" ] || fail 'runuser missing: graded account isolation unavailable'
id "$GRADED_USER" >/dev/null 2>&1 || fail 'graded account missing: isolation unavailable'
chmod -R go-rwx "$TESTS_DIR" || fail 'cannot close the verifier tree to the graded account'

TARGET="$SANDBOX/target"
mkdir -p "$TARGET" "$SANDBOX/results"
chown -R "$GRADED_USER" "$RUNNER" "$TARGET" "$SANDBOX/results"
GRADED_UID="$(/usr/bin/env -i "$RUNUSER" -u "$GRADED_USER" -- /usr/bin/id -u 2>/dev/null)"
[ -n "$GRADED_UID" ] && [ "$GRADED_UID" != 0 ] || fail 'graded account isolation failed'
BUILD=(/usr/bin/env -i PATH="$PATH" HOME="$RUNNER/home" CARGO_HOME="$CARGO_HOME" RUSTUP_HOME=/usr/local/rustup CARGO_NET_OFFLINE=true CARGO_TARGET_DIR="$TARGET" RUSTC="$RUSTC_BIN" "$RUNUSER" -u "$GRADED_USER" -- "$CARGO_BIN")
RUN_PREFIX=(/usr/bin/env -i PATH=/usr/bin:/bin HOME=/nonexistent "$RUNUSER" -u "$GRADED_USER" --)

build_start="$(date +%s)"
if ! "$TIMEOUT_BIN" -k 10 "$BUILD_TIMEOUT" "${BUILD[@]}" test --offline --locked --manifest-path "$RUNNER/Cargo.toml" --test graded --no-run > "$LOGS/build.log" 2>&1; then
    fail 'candidate-linked graded runner did not build (or exceeded its build budget)'
fi
if ! "$TIMEOUT_BIN" -k 10 "$BUILD_TIMEOUT" "${BUILD[@]}" build --offline --locked --manifest-path "$RUNNER/Cargo.toml" --bin frozen_probe >> "$LOGS/build.log" 2>&1; then
    fail 'candidate-linked frozen runner did not build (or exceeded its build budget)'
fi
build_end="$(date +%s)"

GRADED_BIN="$(find "$TARGET/debug/deps" -maxdepth 1 -type f -name 'graded-*' -perm -111 -print -quit)"
FROZEN_BIN="$TARGET/debug/frozen_probe"
[ -x "$GRADED_BIN" ] && [ -x "$FROZEN_BIN" ] || fail 'candidate-linked runner artifacts missing'

run_start="$(date +%s)"
phase_closed=0
while read -r kind name extra; do
    case "$kind" in ''|'#'*) continue ;; esac
    [ -z "${extra:-}" ] || fail 'malformed case manifest'
    if [ "$phase_closed" -eq 1 ]; then
        log "case $name skipped: candidate phase already failed closed"
        continue
    fi
    budget=$((VERIFIER_STARTED + PHASE_BUDGET - $(date +%s)))
    [ "$budget" -gt "$CASE_TIMEOUT" ] && budget="$CASE_TIMEOUT"
    if [ "$budget" -lt 1 ]; then
        log "candidate phase budget of ${PHASE_BUDGET}s exhausted before $name; failing closed"
        phase_closed=1
        continue
    fi
    artifact="$SANDBOX/results/$name"
    stdout="$LOGS/candidate.$name.out"
    rc=0
    if [ "$kind" = graded ]; then
        "$TIMEOUT_BIN" -k 5 "$budget" "${RUN_PREFIX[@]}" /usr/bin/env FILL_OBSERVATION_FILE="$artifact" "$GRADED_BIN" --exact "$name" --test-threads=1 > "$stdout" 2>&1 || rc=$?
    elif [ "$kind" = frozen ]; then
        "$TIMEOUT_BIN" -k 5 "$budget" "${RUN_PREFIX[@]}" "$FROZEN_BIN" "$name" "$artifact" > "$stdout" 2>&1 || rc=$?
    else
        fail 'unknown case kind'
    fi
    log "runner $name exit=$rc budget=${budget}s (diagnostic only)"
    if [ "$rc" -eq 124 ] || [ "$rc" -eq 137 ]; then
        log "case $name exceeded its budget; failing closed, no further candidate code is run"
        rm -f "$artifact"
        phase_closed=1
    fi
done < "$TESTS_DIR/cases.txt"
run_end="$(date +%s)"

# No candidate-linked process runs beyond this point. Whatever a candidate may
# have managed to do to the reward file or the report - write it, replace it
# with a symlink or a directory - is discarded here, unconditionally: both are
# removed and recreated as regular files owned by the verifier.
rm -rf "$REWARD" "$REPORT"
printf '0\n' > "$REWARD"
log "reward reset to 0 after the candidate phase"

# Mandatory verifier-owned expected-failure control. It must observe exactly
# one rejected raw record. Candidate code and output do not participate.
control_ok=0
if "$TIMEOUT_BIN" -k 5 60 "$TESTS_DIR/judge.sh" --selftest "$TESTS_DIR/cases.txt" "$TESTS_DIR/expected" "$TESTS_DIR/expected" "$VERIFY/control-verdict.txt"; then
    control_ok=1
    log 'CONTROL ok'
else
    log 'CONTROL failed'
fi

judge_start="$(date +%s)"
grade_rc=0
"$TIMEOUT_BIN" -k 5 120 "$TESTS_DIR/judge.sh" --grade "$TESTS_DIR/cases.txt" "$TESTS_DIR/expected" "$SANDBOX/results" "$VERIFY/verdict.txt" || grade_rc=$?
judge_end="$(date +%s)"
cp "$VERIFY/verdict.txt" "$LOGS/verdict.txt"

total=0
failed=0
if [ -f "$VERIFY/verdict.txt" ]; then
    read -r total failed < <(awk '/^TOTAL / { print $2, $4 }' "$VERIFY/verdict.txt")
fi
: "${total:=0}"
: "${failed:=0}"
passed=$((total - failed))

rm -rf "$REPORT"
printf '{"reportFormat":"CTRF","results":{"tool":{"name":"fill-verifier-owned"},"summary":{"tests":%s,"passed":%s,"failed":%s},"tests":[]}}\n' "$total" "$passed" "$failed" > "$REPORT"
log "timing runner_build=$((build_end-build_start)) runner_execution=$((run_end-run_start)) judge=$((judge_end-judge_start))"
log "verdict total=$total failed=$failed control=$control_ok reward=0"

if [ "$grade_rc" -eq 0 ] && [ "$total" -eq 53 ] && [ "$failed" -eq 0 ] && [ "$control_ok" -eq 1 ] && [ ! -L "$REWARD" ] && [ -f "$REWARD" ]; then
    printf '1\n' > "$REWARD"
fi
log "final reward=$(cat "$REWARD")"
[ "$(cat "$REWARD")" = 1 ]
