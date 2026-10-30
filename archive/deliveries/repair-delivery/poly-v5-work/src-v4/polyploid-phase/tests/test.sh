#!/usr/bin/env bash
#
# Hardened verifier for the hidden test suite.
#
# Grading is split into two phases so that the candidate's solver runs once per
# distinct input instead of once per assertion group.
#
#   probe   the candidate's solver is compiled into the `probe` binary, and
#           nowhere else. The runner drives the probe plan: one job per entry,
#           each an isolated process that solves one cross, writes one artifact,
#           and exits. Stdout is captured to a log and grades nothing.
#
#   judge   the `judge` binary links no candidate code. It receives the token,
#           the certified answers and the captured artifacts on standard input,
#           and runs the graded checks against those artifacts. A check that
#           wants an answer cannot cause another execution.
#
# The number of probe jobs is fixed by source: `probe_plan::PLAN` in the
# verifier's own crate, cross-checked below against the `job!` lines that define
# it, exactly as the number of checks is cross-checked against the `check!` lines
# in the judge's registry. Execution count is therefore a property of verifier
# source, not an emergent consequence of how many assertions there are.
#
# What the groups A-M validate:
#   A  the document the agent left at /app/result.json holds the optimum
#   B  each parent's homolog rows match parent_dosage at every marker
#   C  inheritance states are ordered pairs naming homologs that exist
#   D  no double reduction where the cross forbids it
#   E  double reduction is used where the optimum contains it
#   F  answers are the canonical representative of their relabelling class
#   G  canonicalisation remaps paths with the rows and is idempotent
#   H  every fixture reaches the exact global optimum
#   I  a marker-at-a-time reconstruction falls short of that optimum
#   J  recombination between markers is part of the objective
#   K  repeated runs agree, and ties resolve to the documented representative
#   L  the compound fixture is solved, and still combines all four difficulties
#   M  answers follow the supplied cross, and grading can reject a wrong one
# No expected value is named in any of that, in the check names, or in any
# failure message; the certified answers never appear on a disk the candidate
# code can reach.
#
# The candidate may not:
#   - control the compiler, the cargo profile, or any build flag;
#   - supply Cargo.toml, Cargo.lock, build.rs, .cargo/config.toml, or a
#     rust-toolchain file that the grading build reads;
#   - inject RUSTFLAGS, RUSTC, LD_PRELOAD, PATH entries or compiler wrappers;
#   - replace the frozen scorer, reader, writer or digest with its own;
#   - plant a passing test binary, a fixture, a certified answer or a prewritten
#     reward file;
#   - discover the hidden fixtures, their expected answers, or this suite;
#   - overwrite /logs/verifier or reach outside /app/src/solver.
#
# Every exploit in that list must fail to earn reward 1.

set -uo pipefail

LOGS=/logs/verifier
REWARD="$LOGS/reward.txt"
REPORT="$LOGS/report.json"

mkdir -p "$LOGS"
echo "0" > "$REWARD"

# -------------------------------------------------------------------------
# Run deadline
#
# The verifier's own timeout is 1800 seconds and a run that reaches it is
# reported as an infrastructure failure with no graded result. Everything below
# is bounded by this deadline instead, so an over-budget candidate produces
# reward 0 with a report naming the checks that failed. The margin covers
# writing the report and the reward after the deadline is hit.
# -------------------------------------------------------------------------

SCRIPT_START=$(date +%s)
RUN_DEADLINE=1500

# Seconds available for the next step: the cap, or whatever is left of the
# deadline, whichever is smaller. Zero means the deadline has passed.
allowance() {
  local cap="$1" left
  left=$((RUN_DEADLINE - ($(date +%s) - SCRIPT_START)))
  if [ "$left" -le 0 ]; then echo 0
  elif [ "$left" -lt "$cap" ]; then echo "$left"
  else echo "$cap"
  fi
}

# -------------------------------------------------------------------------
# Environment scrubbing
# -------------------------------------------------------------------------

unset CC CXX AR LD AS RANLIB STRIP OBJCOPY OBJDUMP NM READELF
unset CFLAGS CXXFLAGS CPPFLAGS LDFLAGS LDLIBS
unset RUSTC RUSTDOC RUSTFLAGS RUSTDOCFLAGS RUST_BACKTRACE RUST_LOG
unset RUSTUP_TOOLCHAIN RUSTUP_HOME RUSTUP_DIST_SERVER RUSTUP_UPDATE_ROOT
unset CARGO CARGO_HOME CARGO_TARGET_DIR CARGO_BUILD_TARGET CARGO_BUILD_RUSTC
unset CARGO_BUILD_RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_JOBS
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset HOME TMPDIR TMP TEMP
unset PHASE_FIXTURE_DIR PHASE_APP_DIR PHASE_RUN_TOKEN PHASE_PROBE_BUDGET

HOME=$(mktemp -d)
export HOME
export PATH="/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin"
export CARGO_HOME=/usr/local/cargo
export RUSTUP_HOME=/usr/local/rustup
export CARGO_NET_OFFLINE=true
export RUST_BACKTRACE=0

# -------------------------------------------------------------------------
# Toolchain resolution
#
# The binaries on PATH in the base image are rustup shims, which choose a
# toolchain from RUSTUP_HOME and RUSTUP_TOOLCHAIN. Those were just scrubbed
# because a candidate must not be able to point them anywhere, so the shims are
# bypassed and the toolchain's own binaries are used directly. RUSTUP_HOME is
# still pinned to a trusted absolute path for the fallback, and because cargo
# consults it when it resolves its own rustc.
# -------------------------------------------------------------------------

# Echoes the first executable named $1 found in the installed toolchain, then in
# the trusted prefixes. Toolchain directories are taken in sorted order so the
# choice does not depend on the order the filesystem returns.
resolve_tool() {
  local name="$1" candidate
  for candidate in $(ls -d /usr/local/rustup/toolchains/*/bin 2>/dev/null | sort); do
    if [ -x "$candidate/$name" ]; then echo "$candidate/$name"; return 0; fi
  done
  for candidate in /usr/local/cargo/bin /usr/local/bin /usr/bin /bin; do
    if [ -x "$candidate/$name" ]; then echo "$candidate/$name"; return 0; fi
  done
  return 1
}

CARGO_BIN=$(resolve_tool cargo)
if [ -z "$CARGO_BIN" ]; then
  echo "FATAL: no cargo found in the installed toolchain or trusted prefixes" >&2
  exit 2
fi

RUSTC_BIN=$(resolve_tool rustc)
if [ -z "$RUSTC_BIN" ]; then
  echo "FATAL: no rustc found in the installed toolchain or trusted prefixes" >&2
  exit 2
fi

export RUSTC="$RUSTC_BIN"

{
  echo "=== toolchain ==="
  echo "cargo = $CARGO_BIN"
  echo "rustc = $RUSTC_BIN"
  "$CARGO_BIN" --version 2>&1
  "$RUSTC_BIN" --version 2>&1
} > "$LOGS/build.log"

# -------------------------------------------------------------------------
# Build directory setup
# -------------------------------------------------------------------------

TESTS=/tests
VERIFIER=/verifier

if [ ! -d "$TESTS/verifier/src" ] || [ ! -d "$TESTS/fixtures" ]; then
  echo "FATAL: verifier image is missing its own sources or fixtures" >&2
  exit 2
fi

rm -rf "$VERIFIER"
mkdir -p "$VERIFIER/build"

# The verifier's crate, its frozen core, its fixtures. All copied out of the
# verifier image, none of it out of /app.
cp -R "$TESTS/verifier/." "$VERIFIER/build/"
cp -R "$TESTS/fixtures" "$VERIFIER/fixtures"

# A stale solver from the verifier image must not survive into the graded
# build; the candidate's is the only one that may be compiled. The candidate
# solver lives under the probe binary, and the build is checked below for any
# second copy anywhere else.
rm -rf "$VERIFIER/build/src/bin/probe/solver.rs" "$VERIFIER/build/src/bin/probe/solver"

# -------------------------------------------------------------------------
# Candidate source allow-list
#
# Exactly src/solver.rs and anything under src/solver/. Nothing else the
# candidate wrote is carried into the graded build, so replacing the scorer,
# the reader, the writer, the digest or the manifest achieves nothing.
# -------------------------------------------------------------------------

APP=/app
if [ ! -d "$APP" ]; then
  echo "FATAL: /app was not transferred to the verifier" >&2
  exit 2
fi

CANDIDATE="$APP/src/solver.rs"

if [ ! -f "$CANDIDATE" ]; then
  echo "FATAL: candidate solver missing at $CANDIDATE" >&2
  exit 2
fi

if [ -L "$CANDIDATE" ]; then
  echo "FATAL: $CANDIDATE is a symlink" >&2
  exit 2
fi

cp "$CANDIDATE" "$VERIFIER/build/src/bin/probe/solver.rs"

if [ -e "$APP/src/solver" ]; then
  if [ -L "$APP/src/solver" ] || [ ! -d "$APP/src/solver" ]; then
    echo "FATAL: $APP/src/solver is not a directory" >&2
    exit 2
  fi
  # Any symlink under the tree is rejected outright rather than followed.
  if [ -n "$(find "$APP/src/solver" -type l -print -quit)" ]; then
    echo "FATAL: symlinks under $APP/src/solver" >&2
    exit 2
  fi
  if [ -n "$(find "$APP/src/solver" -type f ! -name '*.rs' -print -quit)" ]; then
    echo "FATAL: non-Rust files under $APP/src/solver" >&2
    exit 2
  fi
  mkdir -p "$VERIFIER/build/src/bin/probe/solver"
  cp -R "$APP/src/solver/." "$VERIFIER/build/src/bin/probe/solver/"
fi

# The candidate's solver may appear in the graded build only under the probe
# binary. A second copy somewhere else means a stale one survived the wipe or a
# copy landed in a place that would compile it into the judge.
stray=$(find "$VERIFIER/build/src" -path "$VERIFIER/build/src/bin/probe/solver*" -prune -o \
  \( -name 'solver.rs' -o -name 'solver' \) -print -quit 2>/dev/null)
if [ -n "$stray" ]; then
  echo "FATAL: unexpected solver source at $stray" >&2
  exit 2
fi

# -------------------------------------------------------------------------
# Build with verifier-owned configuration
# -------------------------------------------------------------------------

echo "=== build ===" >> "$LOGS/build.log"
"$CARGO_BIN" build \
  --offline \
  --release \
  --manifest-path "$VERIFIER/build/Cargo.toml" \
  --target-dir "$VERIFIER/target" \
  >> "$LOGS/build.log" 2>&1
build_rc=$?

if [ "$build_rc" -ne 0 ]; then
  echo "FATAL: the candidate's solver does not compile against the frozen core" >&2
  tail -60 "$LOGS/build.log" >&2
  exit 2
fi

PROBE_BIN="$VERIFIER/target/release/probe"
JUDGE_BIN="$VERIFIER/target/release/judge"
if [ ! -x "$PROBE_BIN" ] || [ ! -x "$JUDGE_BIN" ]; then
  echo "FATAL: probe or judge binary not produced" >&2
  exit 2
fi

# -------------------------------------------------------------------------
# Enumerate the plan and the checks
# -------------------------------------------------------------------------

mapfile -t JOBS < <("$PROBE_BIN" --list 2>/dev/null)
JOBS_LISTED=${#JOBS[@]}

mapfile -t NAMES < <("$JUDGE_BIN" --list 2>/dev/null)
LISTED=${#NAMES[@]}

# The authoritative counts come from the verifier's own source, not from the
# binaries it produced. A candidate that somehow shrinks the registry cannot
# shrink the number of jobs it must run or checks it must pass.
#
# Both patterns match an invocation that opens a line, so neither depends on how
# deeply its enclosing block happens to be indented. A count of zero means the
# pattern stopped matching the source and is a fatal verifier fault, not a
# candidate result: it would otherwise pass a suite of no checks.
PLAN_SOURCE="$TESTS/verifier/src/probe_plan.rs"
JOBS_EXPECTED=$(grep -cE '^[[:space:]]*job!\(' "$PLAN_SOURCE")

REGISTRY="$TESTS/verifier/src/bin/judge/main.rs"
EXPECTED=$(grep -cE '^[[:space:]]*check!\(' "$REGISTRY")

if [ "$JOBS_EXPECTED" -eq 0 ]; then
  echo "FATAL: could not count probe jobs in the verifier source" >&2
  exit 2
fi

if [ "$JOBS_LISTED" -ne "$JOBS_EXPECTED" ]; then
  echo "FATAL: probe binary lists $JOBS_LISTED jobs, verifier source defines $JOBS_EXPECTED" >&2
  exit 2
fi

if [ "$EXPECTED" -eq 0 ]; then
  echo "FATAL: could not count checks in the verifier source" >&2
  exit 2
fi

if [ "$LISTED" -ne "$EXPECTED" ]; then
  echo "FATAL: judge binary lists $LISTED checks, verifier source defines $EXPECTED" >&2
  exit 2
fi

echo "=== $JOBS_EXPECTED probe jobs, $EXPECTED checks ===" > "$LOGS/run.log"

# -------------------------------------------------------------------------
# Ground truth capture and removal
#
# The certified answers are read into a shell variable and then removed from
# every filesystem, before the first probe starts. A probe runs candidate code
# with this script's privileges, so a solver that can read a certified answer
# can replay it instead of searching, and no directory is safe from it. They are
# handed to the judge on the closed stdin channel, together with the token and
# the captured artifacts.
#
# The crosses stay on disk. A cross is an input the candidate is meant to solve;
# reading one gains it nothing it was not already given.
# -------------------------------------------------------------------------

expected_count=0
for f in "$VERIFIER/fixtures"/*.expected.json; do
  [ -f "$f" ] && expected_count=$((expected_count + 1))
done

if [ "$expected_count" -eq 0 ]; then
  echo "FATAL: the verifier image carries no certified answers" >&2
  exit 2
fi

# Assembled into a shell variable rather than a file. A file would be readable
# by any process that could reach it, which is the thing this is preventing.
build_expectations() {
  for f in "$VERIFIER/fixtures"/*.expected.json; do
    [ -f "$f" ] || continue
    stem=$(basename "$f" .expected.json)
    bytes=$(wc -c < "$f" | tr -d ' ')
    printf '##FIXTURE %s %s\n' "$stem" "$bytes"
    cat "$f"
    printf '\n'
  done
}

# The sentinel is what makes the byte counts above trustworthy. Command
# substitution strips every trailing newline, and each count came from `wc -c`
# over the whole file, so a certified answer that happened to end in a newline
# would arrive shorter than its header claims and the judge would reject the
# payload as truncated. A non-newline last character means there is nothing to
# strip; removing it afterwards restores the frame exactly.
EXPECTATIONS=$(build_expectations; printf '.')
EXPECTATIONS=${EXPECTATIONS%.}

rm -f "$VERIFIER/fixtures"/*.expected.json
rm -f "$TESTS/fixtures"/*.expected.json

leftover=$(find "$VERIFIER/fixtures" "$TESTS/fixtures" -name '*.expected.json' -print -quit 2>/dev/null)
if [ -n "$leftover" ]; then
  echo "FATAL: certified answers are still readable at $leftover" >&2
  exit 2
fi

# -------------------------------------------------------------------------
# Probe phase: one candidate execution per job, nothing more
# -------------------------------------------------------------------------

# A per-run token the candidate cannot predict. A passing check must echo it
# back on a marker line, so a process that exits successfully without running
# the check it was given does not count as a pass. Delivered to the judge on
# standard input, never exported: an environment variable would be visible to
# every process this script starts, including the probes.
TOKEN=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')

# Bounds one candidate execution. Clamped by the run deadline, so the phase
# cannot consume the time the judge needs. Both bounds fail closed: a job that
# times out or is never reached leaves its artifact missing, and the checks that
# wanted it fail with a reason rather than the run dying without a report.
PER_PROBE_TIMEOUT=300

export PHASE_FIXTURE_DIR="$VERIFIER/fixtures"

# Written by the probe, read by this script, wiped before every job. A probe
# could write files it was not asked for — it runs candidate code — so its
# artifact is captured into memory the moment it exits and the path is cleared
# before the next job. A later probe therefore cannot reach an earlier job's
# captured bytes, which is what stops one execution forging another's answer.
STAGING="$VERIFIER/staging"
ARTIFACT="$STAGING/current.artifact.json"
mkdir -p "$STAGING"

ARTIFACT_FRAMES=""
probe_started=$(date +%s)
probe_ok=0
probe_failed=0
probe_slowest=0
probe_slowest_job="none"

for job in "${JOBS[@]}"; do
  rm -f "$ARTIFACT"
  OUT="$LOGS/probe.$job.txt"

  budget=$(allowance "$PER_PROBE_TIMEOUT")
  if [ "$budget" -le 0 ]; then
    echo "FAIL probe $job (run deadline reached)" >> "$LOGS/run.log"
    probe_failed=$((probe_failed + 1))
    continue
  fi

  start=$(date +%s)
  rc=0
  timeout "$budget" "$PROBE_BIN" "$job" "$ARTIFACT" > "$OUT" 2>&1 || rc=$?
  elapsed=$(($(date +%s) - start))

  if [ "$elapsed" -gt "$probe_slowest" ]; then
    probe_slowest=$elapsed
    probe_slowest_job="$job"
  fi

  if [ "$rc" -eq 0 ] && [ -s "$ARTIFACT" ]; then
    body=$(cat "$ARTIFACT")
    bytes=$(printf '%s' "$body" | wc -c | tr -d ' ')
    ARTIFACT_FRAMES+="##ARTIFACT $job $bytes"$'\n'"$body"$'\n'
    echo "PASS probe $job (${elapsed}s)" >> "$LOGS/run.log"
    probe_ok=$((probe_ok + 1))
  else
    echo "FAIL probe $job (rc=$rc, ${elapsed}s)" >> "$LOGS/run.log"
    probe_failed=$((probe_failed + 1))
  fi
done

rm -rf "$STAGING"
PROBE_SECONDS=$(($(date +%s) - probe_started))

# A probe that failed is a candidate failure, not a verifier fault: the run
# continues, the checks that wanted that answer fail, and the report says so.
echo "probe phase: $probe_ok of $JOBS_EXPECTED jobs in ${PROBE_SECONDS}s, \
slowest $probe_slowest_job at ${probe_slowest}s" >> "$LOGS/run.log"

# -------------------------------------------------------------------------
# Judge phase: graded checks, one process each
# -------------------------------------------------------------------------

PAYLOAD="$TOKEN"$'\n'"$EXPECTATIONS"$'\n'"$ARTIFACT_FRAMES"'##END'$'\n'
unset EXPECTATIONS ARTIFACT_FRAMES

export PHASE_APP_DIR="$APP"

PASS_MARKER="##PHASE-PASS"
CONTROL="negative_control_must_fail"
PER_CHECK_TIMEOUT=60

passed=0
failures=()

for name in "${NAMES[@]}"; do
  OUT="$LOGS/out.$name.txt"
  rc=0
  budget=$(allowance "$PER_CHECK_TIMEOUT")
  if [ "$budget" -le 0 ]; then
    failures+=("$name")
    echo "FAIL $name (run deadline reached)" >> "$LOGS/run.log"
    continue
  fi
  printf '%s' "$PAYLOAD" | timeout "$budget" "$JUDGE_BIN" --run "$name" > "$OUT" 2>&1 || rc=$?

  if [ "$rc" -eq 0 ] && grep -qxF "$PASS_MARKER $name $TOKEN" "$OUT"; then
    passed=$((passed + 1))
    echo "PASS $name" >> "$LOGS/run.log"
  else
    failures+=("$name")
    echo "FAIL $name (rc=$rc)" >> "$LOGS/run.log"
  fi
done

# The negative control must fail. If it passes, results are being forged and
# no reward is given whatever the graded checks reported. A control that could
# not be run at all is not evidence that failure is detected, so it stays 0.
control_ok=0
CONTROL_OUT="$LOGS/out.$CONTROL.txt"
control_rc=0
control_budget=$(allowance "$PER_CHECK_TIMEOUT")
if [ "$control_budget" -gt 0 ]; then
  printf '%s' "$PAYLOAD" | timeout "$control_budget" "$JUDGE_BIN" --run "$CONTROL" \
    > "$CONTROL_OUT" 2>&1 || control_rc=$?

  if [ "$control_rc" -ne 0 ] && ! grep -qxF "$PASS_MARKER $CONTROL $TOKEN" "$CONTROL_OUT"; then
    control_ok=1
    echo "CONTROL ok (failed as required)" >> "$LOGS/run.log"
  else
    echo "CONTROL COMPROMISED (rc=$control_rc)" >> "$LOGS/run.log"
  fi
else
  echo "CONTROL NOT RUN (run deadline reached)" >> "$LOGS/run.log"
fi

echo "passed=$passed graded=$EXPECTED control_ok=$control_ok" >> "$LOGS/run.log"

# -------------------------------------------------------------------------
# CTRF report
# -------------------------------------------------------------------------

STAMP=$(date +%s)
FAILED=$((EXPECTED - passed))
cat > "$REPORT" <<CTRF
{
  "results": {
    "tool": {
      "name": "polyploid-phase-graded"
    },
    "summary": {
      "tests": $EXPECTED,
      "passed": $passed,
      "failed": $FAILED,
      "pending": 0,
      "skipped": 0,
      "other": 0,
      "start": ${STAMP}000,
      "stop": ${STAMP}000
    },
    "tests": []
  }
}
CTRF

# -------------------------------------------------------------------------
# Reward decision
#
# Written last, and written unconditionally, so that anything the candidate's
# code may have done to reward.txt while the checks ran is overwritten here.
# A run in which no check executed is a failed run, not a vacuous pass.
# -------------------------------------------------------------------------

if [ "$passed" -eq "$EXPECTED" ] && [ "$EXPECTED" -gt 0 ] && [ "$control_ok" -eq 1 ]; then
  echo "1" > "$REWARD"
  echo "REWARD: 1 (all $EXPECTED checks passed, control intact)" >> "$LOGS/run.log"
else
  echo "0" > "$REWARD"
  if [ "$control_ok" -ne 1 ]; then
    echo "REWARD: 0 (negative control compromised)" >> "$LOGS/run.log"
  else
    echo "REWARD: 0 ($passed/$EXPECTED passed)" >> "$LOGS/run.log"
  fi
  if [ "${#failures[@]}" -gt 0 ]; then
    echo "--- failures ---" >> "$LOGS/run.log"
    for n in "${failures[@]}"; do
      echo "  $n" >> "$LOGS/run.log"
    done
  fi
fi

exit 0
