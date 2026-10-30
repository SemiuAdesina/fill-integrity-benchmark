#!/usr/bin/env bash
#
# Verifier entry point. Reward is owned here and starts at 0.
#
# Trust model: the graded process is never asked whether it passed. It runs
# unprivileged, links the candidate implementation, and writes nothing but the
# raw functional values its calls produced. A separate judge binary that never
# links candidate code compares those values against expectations it holds and
# alone decides correctness. Candidate stdout, stderr and exit status are
# captured as diagnostics and are read by no grading decision.
set -uo pipefail

# ---------------------------------------------------------------- environment
# Nothing the candidate could have exported may influence the authoritative
# toolchain, its lookup paths, its wrappers or its output locations.
export PATH=/usr/local/bin:/usr/bin:/usr/sbin:/sbin:/bin
unset CC CXX CFLAGS CXXFLAGS CPPFLAGS LDFLAGS
unset CMAKE_TOOLCHAIN_FILE CMAKE_PREFIX_PATH CMAKE_GENERATOR
unset CMAKE_BUILD_TYPE CMAKE_CXX_COMPILER_LAUNCHER CMAKE_MAKE_PROGRAM
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset ASAN_OPTIONS UBSAN_OPTIONS LSAN_OPTIONS MSAN_OPTIONS TSAN_OPTIONS
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY BASH_ENV ENV CDPATH

LOGS=/logs/verifier
REWARD="$LOGS/reward.txt"
REPORT="$LOGS/report.json"
mkdir -p "$LOGS"
printf '0\n' > "$REWARD"
: > "$LOGS/run.log"
log(){ printf '%s\n' "$*" >> "$LOGS/run.log"; }

# Verifier-owned sources must not be readable by the account the graded process
# runs under, so the expected results cannot simply be read off the filesystem.
chmod -R go-rwx /tests 2>/dev/null

RUNUSER=/usr/sbin/runuser
GRADED_USER=nobody
[ -x "$RUNUSER" ] || { log 'FATAL missing runuser; refusing to run graded code with verifier privileges'; exit 2; }
id "$GRADED_USER" >/dev/null 2>&1 || { log 'FATAL missing unprivileged account'; exit 2; }
# Prove the drop actually takes effect before any candidate code is executed.
GRADED_UID=$(env -i "$RUNUSER" -u "$GRADED_USER" -- id -u 2>/dev/null)
[ -n "$GRADED_UID" ] && [ "$GRADED_UID" != 0 ] || { log 'FATAL privilege drop ineffective'; exit 2; }
log "graded process account=$GRADED_USER uid=$GRADED_UID"

# Absolute, trusted toolchain. Resolved here and passed to cmake explicitly so
# no candidate-visible variable or toolchain file can redirect the compiler.
CXXBIN=/usr/local/bin/clang++
[ -x "$CXXBIN" ] || CXXBIN=/usr/bin/clang++
CMAKE=/usr/local/bin/cmake
[ -x "$CMAKE" ] || CMAKE=/usr/bin/cmake
[ -x "$CXXBIN" ] && [ -x "$CMAKE" ] || { log 'FATAL trusted toolchain missing'; exit 2; }
log "toolchain compiler=$CXXBIN cmake=$CMAKE"

# Verifier-private build tree (root, 0700) and a separate world-traversable
# sandbox that holds only what the graded process is allowed to see.
VERIFY=$(mktemp -d /tmp/fabric-verify.XXXXXXXX) || exit 2
SANDBOX=$(mktemp -d /tmp/fabric-sandbox.XXXXXXXX) || exit 2
trap 'rm -rf "$VERIFY" "$SANDBOX"' EXIT
chmod 700 "$VERIFY"
chmod 755 "$SANDBOX"
export HOME="$VERIFY/home"
export TMPDIR="$VERIFY/tmp"
mkdir -p "$HOME" "$TMPDIR"

# ------------------------------------------------------------- source intake
# Allowlist: exactly one candidate file is consumed, the implementation the
# published contract asks the agent to repair -- the same single file V1 read.
# No candidate manifest, cmake config, toolchain file, build script, harness or
# prebuilt binary is read or executed.
ALLOWED=/app/src/recover.cpp
[ -f "$ALLOWED" ] && [ ! -L "$ALLOWED" ] || { log 'FATAL allowlisted implementation missing or not a regular file'; exit 2; }
if find /app -type l -print -quit | grep -q .; then log 'FATAL symlink present under /app'; exit 2; fi

cp -r /tests/workspace "$VERIFY/ws" || exit 2
mkdir -p "$VERIFY/ws/candidate"
cp "$ALLOWED" "$VERIFY/ws/candidate/recover.cpp" || exit 2
cmp -s "$ALLOWED" "$VERIFY/ws/candidate/recover.cpp" || { log 'FATAL candidate intake did not take effect'; exit 2; }

# --------------------------------------------------------------------- build
# Built from the verifier's own manifest, with the compiler pinned to an
# absolute path. The candidate cannot add sources, change flags, inject a
# toolchain file or introduce a build step.
if ! "$CMAKE" -S "$VERIFY/ws" -B "$VERIFY/build" \
     -DCMAKE_CXX_COMPILER="$CXXBIN" \
     > "$LOGS/build.log" 2>&1; then
  log 'FAIL verifier build tree failed to configure'
  exit 2
fi
if ! "$CMAKE" --build "$VERIFY/build" -j 2 >> "$LOGS/build.log" 2>&1; then
  log 'FAIL candidate implementation does not build against the frozen contract'
  exit 2
fi
PROBE="$VERIFY/build/probe"
LIAR="$VERIFY/build/liar_probe"
JUDGE="$VERIFY/build/judge"
for b in "$PROBE" "$LIAR" "$JUDGE"; do
  [ -x "$b" ] || { log "FATAL missing build artifact $b"; exit 2; }
done

# The judge binary stays inside the 0700 verifier tree. Only the probes are
# published into the sandbox, and they carry fixtures but no expected results.
install -m 0755 "$PROBE" "$SANDBOX/probe"
install -m 0755 "$LIAR" "$SANDBOX/liar_probe"
mkdir -p "$SANDBOX/results" "$SANDBOX/liar-results"
chown "$GRADED_USER" "$SANDBOX/results" "$SANDBOX/liar-results"

# The V1 case population, unchanged in name and count. V1's twelfth
# registration was an in-process `REQUIRE(false)` negative control; it is
# superseded by `judge --selftest`, which is decided inside the verifier.
CASES=(saturating_operator_boundaries
       replay_combine_reduces_link_bytes
       replay_without_combine_serializes
       accumulator_exact_boundary
       recover_from_link_and_completion_observations
       reject_non_tree_route_selection
       canonical_equal_route_tie
       global_observation_beats_first_route
       equal_time_minimizes_total_link_bytes
       exact_ready_time_scheduling
       saturating_combine_operation_order
       compound_capacity_symmetry_route_and_replay)
EXPECTED_TOTAL=${#CASES[@]}

# Run one probe case as the unprivileged account in a cleared environment. The
# sanitizer settings are the verifier's, passed explicitly because the
# environment is emptied first. The exit status is recorded for diagnostics and
# deliberately not acted upon.
run_probe(){ # <binary> <results-dir> <case> <stdout-log>
  local rc=0
  env -i PATH=/usr/bin:/bin HOME=/nonexistent \
    ASAN_OPTIONS=detect_leaks=0:abort_on_error=0 \
    UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1 \
    "$RUNUSER" -u "$GRADED_USER" -- "$1" "$3" "$2/$3" > "$4" 2>&1 || rc=$?
  log "probe $3 exit=$rc (status not used for grading)"
}

# ------------------------------------------------ required negative control
# Verifier-owned: the judge stages a full set of correct records, perturbs
# exactly one functional value and must observe exactly that one failure. No
# candidate code participates and no marker is exchanged.
control_ok=0
if "$JUDGE" --selftest "$VERIFY/control" >> "$LOGS/run.log" 2>&1; then
  control_ok=1
  log 'CONTROL ok'
else
  log 'CONTROL compromised'
fi

# -------------------------------------- stdout-authority regression (benign)
# A functionally incorrect implementation that announces success in every
# plausible form must still be graded as incorrect.
regression_ok=0
for c in "${CASES[@]}"; do
  run_probe "$SANDBOX/liar_probe" "$SANDBOX/liar-results" "$c" "$SANDBOX/liar.$c.out"
done
if "$JUDGE" --grade "$SANDBOX/liar-results" "$VERIFY/liar-verdict.txt" >> "$LOGS/run.log" 2>&1; then
  log 'REGRESSION compromised: success-looking output was treated as correctness'
else
  regression_ok=1
  log 'REGRESSION ok: emitted output carried no grading authority'
fi

# ------------------------------------------------------ graded candidate run
for c in "${CASES[@]}"; do
  run_probe "$SANDBOX/probe" "$SANDBOX/results" "$c" "$LOGS/candidate.$c.out"
done

grade_rc=0
"$JUDGE" --grade "$SANDBOX/results" "$VERIFY/verdict.txt" >> "$LOGS/run.log" 2>&1 || grade_rc=$?

TOTAL=0; FAILED=0
if [ -f "$VERIFY/verdict.txt" ]; then
  read -r TOTAL FAILED < <(awk '/^TOTAL /{print $2, $4}' "$VERIFY/verdict.txt")
fi
: "${TOTAL:=0}"; : "${FAILED:=0}"
PASSED=$((TOTAL-FAILED))

printf '{"reportFormat":"CTRF","results":{"tool":{"name":"reduction-plan-verifier-owned"},"summary":{"tests":%s,"passed":%s,"failed":%s},"tests":[]}}\n' \
  "$TOTAL" "$PASSED" "$FAILED" > "$REPORT"

# Reward is awarded only when every verifier-owned functional check passed, the
# expected case population was actually evaluated, the negative control saw its
# injected failure, and the stdout-authority regression stayed at zero.
if [ "$grade_rc" -eq 0 ] && [ "$TOTAL" -eq "$EXPECTED_TOTAL" ] && [ "$FAILED" -eq 0 ] \
   && [ "$control_ok" -eq 1 ] && [ "$regression_ok" -eq 1 ]; then
  printf '1\n' > "$REWARD"
fi

log "verdict total=$TOTAL failed=$FAILED control=$control_ok regression=$regression_ok reward=$(cat "$REWARD")"
[ "$(cat "$REWARD")" = 1 ]
