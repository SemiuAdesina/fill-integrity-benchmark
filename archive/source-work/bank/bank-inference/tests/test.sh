#!/usr/bin/env bash
#
# Verifier entry point. Reward is owned here and starts at 0.
#
# Trust model: the graded process is never asked whether it passed. It runs
# unprivileged, links the candidate recovery routine, and writes nothing but the
# raw functional values its call produced. A separate judge binary that never
# links candidate code compares those values against expectations it holds and
# derives itself from the frozen interpreter, and it alone decides correctness.
# Candidate stdout, stderr and exit status are captured as diagnostics and are
# read by no grading decision.
set -uo pipefail

# ---------------------------------------------------------------- environment
# Nothing the candidate could have exported may influence the authoritative
# toolchain, its lookup paths, its wrappers or its output locations.
export PATH=/usr/local/bin:/usr/bin:/usr/sbin:/sbin:/bin
unset CC CXX CFLAGS CXXFLAGS CPPFLAGS LDFLAGS
unset CMAKE_TOOLCHAIN_FILE CMAKE_PREFIX_PATH CMAKE_GENERATOR
unset CMAKE_BUILD_TYPE CMAKE_CXX_COMPILER_LAUNCHER CMAKE_MAKE_PROGRAM
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset HTTP_PROXY HTTPS_PROXY ALL_PROXY NO_PROXY BASH_ENV ENV CDPATH

LOGS=/logs/verifier
REWARD="$LOGS/reward.txt"
REPORT="$LOGS/report.json"
mkdir -p "$LOGS"
printf '0\n' > "$REWARD"
: > "$LOGS/run.log"
log(){ printf '%s\n' "$*" >> "$LOGS/run.log"; }
total_started=$(date +%s%N)

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
VERIFY=$(mktemp -d /tmp/bank-verify.XXXXXXXX)
SANDBOX=$(mktemp -d /tmp/bank-sandbox.XXXXXXXX)
trap 'rm -rf "$VERIFY" "$SANDBOX"' EXIT
chmod 700 "$VERIFY"
chmod 755 "$SANDBOX"
export HOME="$VERIFY/home"
export TMPDIR="$VERIFY/tmp"
mkdir -p "$HOME" "$TMPDIR"

# ------------------------------------------------------------- source intake
# Allowlist: exactly one candidate file is consumed, the recovery routine the
# published contract asks the agent to repair -- the same single file V1 read.
# No candidate manifest, cmake config, toolchain file, build script, harness or
# prebuilt binary is read or executed.
ALLOWED=/app/src/recover.cpp
[ -f "$ALLOWED" ] && [ ! -L "$ALLOWED" ] || { log 'FATAL allowlisted recovery source missing or not a regular file'; exit 2; }
if find /app -type l -print -quit | grep -q .; then log 'FATAL symlink present under /app'; exit 2; fi

cp -r /tests/workspace "$VERIFY/ws"
mkdir -p "$VERIFY/ws/candidate"
cp "$ALLOWED" "$VERIFY/ws/candidate/recover.cpp"
cmp -s "$ALLOWED" "$VERIFY/ws/candidate/recover.cpp" || { log 'FATAL candidate intake did not take effect'; exit 2; }

# --------------------------------------------------------------------- build
# Built from the verifier's own manifest, with the compiler pinned to an
# absolute path. The candidate cannot add sources, change flags, inject a
# toolchain file or introduce a build step.
build_started=$(date +%s%N)
if ! "$CMAKE" -S "$VERIFY/ws" -B "$VERIFY/build" \
     -DCMAKE_CXX_COMPILER="$CXXBIN" \
     > "$LOGS/build.log" 2>&1; then
  log 'FAIL verifier build tree failed to configure'
  exit 2
fi
if ! "$CMAKE" --build "$VERIFY/build" -j 2 >> "$LOGS/build.log" 2>&1; then
  log 'FAIL candidate recovery does not build against the frozen interpreter'
  exit 2
fi
build_ended=$(date +%s%N)
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

# The graded case population. `compound_global_tie` is the compound case that
# requires every published mapping rule to hold at once.
CASES=(basic_identity word_boundaries broadcast_groups
       conflict_disambiguates ordered_xor ordered_xor_reverse_declaration
       minimum_stage_count_tie global_evidence
       minimum_canonical compound_replay compound_global_tie no_solution)
EXPECTED_TOTAL=${#CASES[@]}

# Run one probe case as the unprivileged account in a cleared environment. The
# exit status is recorded for diagnostics and deliberately not acted upon.
run_probe(){ # <binary> <results-dir> <case> <stdout-log>
  local rc=0
  env -i PATH=/usr/bin:/bin HOME=/nonexistent \
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
# A functionally incorrect recovery that announces success in every plausible
# form must still be graded as incorrect.
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
runner_started=$(date +%s%N)
for c in "${CASES[@]}"; do
  run_probe "$SANDBOX/probe" "$SANDBOX/results" "$c" "$LOGS/candidate.$c.out"
done
runner_ended=$(date +%s%N)

grade_rc=0
judge_started=$(date +%s%N)
"$JUDGE" --grade "$SANDBOX/results" "$VERIFY/verdict.txt" >> "$LOGS/run.log" 2>&1 || grade_rc=$?
judge_ended=$(date +%s%N)

TOTAL=0; FAILED=0
if [ -f "$VERIFY/verdict.txt" ]; then
  read -r TOTAL FAILED < <(awk '/^TOTAL /{print $2, $4}' "$VERIFY/verdict.txt")
fi
: "${TOTAL:=0}"; : "${FAILED:=0}"
PASSED=$((TOTAL-FAILED))

printf '{"reportFormat":"CTRF","results":{"tool":{"name":"bank-inference-verifier-owned"},"summary":{"tests":%s,"passed":%s,"failed":%s},"tests":[]}}\n' \
  "$TOTAL" "$PASSED" "$FAILED" > "$REPORT"

# Reward is awarded only when every verifier-owned functional check passed, the
# expected case population was actually evaluated, the negative control saw its
# injected failure, and the stdout-authority regression stayed at zero.
if [ "$grade_rc" -eq 0 ] && [ "$TOTAL" -eq "$EXPECTED_TOTAL" ] && [ "$FAILED" -eq 0 ] \
   && [ "$control_ok" -eq 1 ] && [ "$regression_ok" -eq 1 ]; then
  printf '1\n' > "$REWARD"
fi

total_ended=$(date +%s%N)
log "timing_ms build=$(((build_ended-build_started)/1000000)) runner=$(((runner_ended-runner_started)/1000000)) judge=$(((judge_ended-judge_started)/1000000)) total=$(((total_ended-total_started)/1000000))"
log "verdict total=$TOTAL failed=$FAILED control=$control_ok regression=$regression_ok reward=$(cat "$REWARD")"
printf 'verifier: total=%s failed=%s control=%s regression=%s reward=%s\n' \
  "$TOTAL" "$FAILED" "$control_ok" "$regression_ok" "$(tr -d '\n' < "$REWARD")"
[ "$(cat "$REWARD")" = 1 ]
