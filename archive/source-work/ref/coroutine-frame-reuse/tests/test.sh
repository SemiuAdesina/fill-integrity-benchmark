#!/usr/bin/env bash
#
# Hardened verifier for the hidden test suite.
#
# The candidate may not:
#   - control the compiler, linker, build flags, or sanitizer options;
#   - supply CMakeLists.txt, build scripts, or toolchain files;
#   - inject LD_PRELOAD, PATH manipulation, or compiler wrappers;
#   - plant passing test binaries or prewritten reward files;
#   - discover the hidden tests, reference solution, or expected outputs;
#   - overwrite /logs/verifier or escape /app/src.
#
# Every exploit in that list must fail to earn reward 1.

set -uo pipefail

LOGS=/logs/verifier
REWARD="$LOGS/reward.txt"
REPORT="$LOGS/report.json"

mkdir -p "$LOGS"
echo "0" > "$REWARD"

# -------------------------------------------------------------------------
# Environment scrubbing
# -------------------------------------------------------------------------

unset CC CXX AR LD AS RANLIB STRIP OBJCOPY OBJDUMP NM READELF
unset CFLAGS CXXFLAGS CPPFLAGS LDFLAGS LDLIBS
unset CMAKE_C_COMPILER CMAKE_CXX_COMPILER CMAKE_AR CMAKE_LINKER
unset CMAKE_C_FLAGS CMAKE_CXX_FLAGS CMAKE_EXE_LINKER_FLAGS CMAKE_SHARED_LINKER_FLAGS
unset CMAKE_MODULE_LINKER_FLAGS CMAKE_STATIC_LINKER_FLAGS
unset CMAKE_PREFIX_PATH CMAKE_TOOLCHAIN_FILE CMAKE_GENERATOR CMAKE_MAKE_PROGRAM
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset ASAN_OPTIONS UBSAN_OPTIONS LSAN_OPTIONS MSAN_OPTIONS TSAN_OPTIONS
unset HOME TMPDIR TMP TEMP

export HOME=$(mktemp -d)
export PATH="/usr/local/bin:/usr/bin:/bin"

# Trusted sanitizer options. The candidate cannot influence these.
export ASAN_OPTIONS="detect_leaks=1:abort_on_error=0:allocator_may_return_null=0:detect_stack_use_after_return=1"
export UBSAN_OPTIONS="print_stacktrace=1:halt_on_error=1"
export LSAN_OPTIONS="report_objects=1"

# -------------------------------------------------------------------------
# Toolchain resolution
# -------------------------------------------------------------------------

CLANG=""
CLANGXX=""
for prefix in /usr/local/bin /usr/bin /bin /usr/local/llvm*/bin /opt/llvm*/bin; do
  if [ -x "$prefix/clang" ]; then CLANG="$prefix/clang"; fi
  if [ -x "$prefix/clang++" ]; then CLANGXX="$prefix/clang++"; fi
  if [ -n "$CLANG" ] && [ -n "$CLANGXX" ]; then break; fi
done

if [ -z "$CLANG" ] || [ -z "$CLANGXX" ]; then
  echo "FATAL: no clang/clang++ found in trusted prefixes" >&2
  exit 2
fi

CMAKE=""
for prefix in /usr/local/bin /usr/bin /bin; do
  if [ -x "$prefix/cmake" ]; then CMAKE="$prefix/cmake"; break; fi
done

if [ -z "$CMAKE" ]; then
  echo "FATAL: no cmake found in trusted prefixes" >&2
  exit 2
fi

echo "=== toolchain ===" > "$LOGS/build.log"
echo "clang    = $CLANG" >> "$LOGS/build.log"
echo "clang++  = $CLANGXX" >> "$LOGS/build.log"
echo "cmake    = $CMAKE" >> "$LOGS/build.log"
"$CLANG" --version >> "$LOGS/build.log" 2>&1 || true
"$CMAKE" --version >> "$LOGS/build.log" 2>&1 || true

# -------------------------------------------------------------------------
# Build directory setup
# -------------------------------------------------------------------------

VERIFIER=/verifier
rm -rf "$VERIFIER"
mkdir -p "$VERIFIER/frozen/include/roundel" "$VERIFIER/frozen/src"
mkdir -p "$VERIFIER/candidate/include/roundel" "$VERIFIER/candidate/src"

FROZEN=/tests/frozen
TESTS=/tests

if [ ! -d "$FROZEN" ] || [ ! -f "$TESTS/graded.cpp" ]; then
  echo "FATAL: verifier image missing frozen sources or graded.cpp" >&2
  exit 2
fi

cp "$FROZEN/include/roundel/journal.hpp" "$VERIFIER/frozen/include/roundel/"
cp "$FROZEN/include/roundel/arena.hpp"   "$VERIFIER/frozen/include/roundel/"
cp "$FROZEN/src/journal.cpp"             "$VERIFIER/frozen/src/"
cp "$FROZEN/src/arena.cpp"               "$VERIFIER/frozen/src/"

cp "$TESTS/graded.cpp"      "$VERIFIER/"
cp "$TESTS/harness.hpp"     "$VERIFIER/"
cp "$TESTS/CMakeLists.txt"  "$VERIFIER/"

# -------------------------------------------------------------------------
# Candidate source allow-list
# -------------------------------------------------------------------------

APP=/app
if [ ! -d "$APP" ]; then
  echo "FATAL: /app not transferred to verifier" >&2
  exit 2
fi

CANDIDATE_HPP="$APP/include/roundel/runtime.hpp"
CANDIDATE_CPP="$APP/src/runtime.cpp"

if [ ! -f "$CANDIDATE_HPP" ]; then
  echo "FATAL: candidate runtime.hpp missing at $CANDIDATE_HPP" >&2
  exit 2
fi

if [ ! -f "$CANDIDATE_CPP" ]; then
  echo "FATAL: candidate runtime.cpp missing at $CANDIDATE_CPP" >&2
  exit 2
fi

# Reject symlinks attempting to escape /app/src.
if [ -L "$CANDIDATE_HPP" ] || [ -L "$CANDIDATE_CPP" ]; then
  echo "FATAL: candidate sources are symlinks" >&2
  exit 2
fi

cp "$CANDIDATE_HPP" "$VERIFIER/candidate/include/roundel/"
cp "$CANDIDATE_CPP" "$VERIFIER/candidate/src/"

# -------------------------------------------------------------------------
# Build with verifier-owned configuration
# -------------------------------------------------------------------------

echo "=== configure ===" >> "$LOGS/build.log"
"$CMAKE" -S "$VERIFIER" -B "$VERIFIER/build" \
  -DCMAKE_BUILD_TYPE=Debug \
  -DCMAKE_C_COMPILER="$CLANG" \
  -DCMAKE_CXX_COMPILER="$CLANGXX" \
  >> "$LOGS/build.log" 2>&1

if [ $? -ne 0 ]; then
  echo "FATAL: configure failed" >&2
  tail -40 "$LOGS/build.log" >&2
  exit 2
fi

echo "=== build ===" >> "$LOGS/build.log"
"$CMAKE" --build "$VERIFIER/build" -j 4 >> "$LOGS/build.log" 2>&1

if [ $? -ne 0 ]; then
  echo "FATAL: build failed" >&2
  tail -60 "$LOGS/build.log" >&2
  exit 2
fi

BIN="$VERIFIER/build/roundel_tests"
if [ ! -x "$BIN" ]; then
  echo "FATAL: test binary not produced" >&2
  exit 2
fi

# -------------------------------------------------------------------------
# Enumerate tests from the verifier's own binary
# -------------------------------------------------------------------------

mapfile -t NAMES < <("$BIN" --list 2>/dev/null)
LISTED=${#NAMES[@]}

# The authoritative count comes from the verifier's own test source, not from
# the binary. A candidate that somehow shrinks the registry cannot shrink the
# number of tests it is required to pass.
EXPECTED=$(grep -c '^ROUNDEL_TEST(' "$TESTS/graded.cpp")

if [ "$EXPECTED" -eq 0 ]; then
  echo "FATAL: could not count tests in verifier source" >&2
  exit 2
fi

if [ "$LISTED" -ne "$EXPECTED" ]; then
  echo "FATAL: binary lists $LISTED tests, verifier source defines $EXPECTED" >&2
  exit 2
fi

echo "=== $EXPECTED tests ===" > "$LOGS/run.log"

# -------------------------------------------------------------------------
# Run each test in its own process
# -------------------------------------------------------------------------

# A per-run token the candidate cannot predict. A passing test must echo it
# back on a marker line, so a process that exits successfully without running
# the test it was given does not count as a pass.
TOKEN=$(head -c 16 /dev/urandom | od -An -tx1 | tr -d ' \n')
export ROUNDEL_RUN_TOKEN="$TOKEN"

# The negative control must fail. If it passes, results are being forged and
# no reward is given whatever the other tests report.
CONTROL="zz_negative_control_must_fail"

passed=0
failures=()
control_ok=0

for name in "${NAMES[@]}"; do
  OUT="$LOGS/out.$name.txt"
  rc=0
  "$BIN" --run "$name" > "$OUT" 2>&1 || rc=$?

  if [ "$name" = "$CONTROL" ]; then
    if [ "$rc" -ne 0 ] && ! grep -qxF "##ROUNDEL-PASS $name $TOKEN" "$OUT"; then
      control_ok=1
      echo "CONTROL ok (failed as required) $name" >> "$LOGS/run.log"
    else
      echo "CONTROL COMPROMISED $name" >> "$LOGS/run.log"
    fi
    continue
  fi

  if [ "$rc" -eq 0 ] && grep -qxF "##ROUNDEL-PASS $name $TOKEN" "$OUT"; then
    passed=$((passed + 1))
    echo "PASS $name" >> "$LOGS/run.log"
  else
    failures+=("$name")
    echo "FAIL $name (rc=$rc)" >> "$LOGS/run.log"
  fi
done

# The control is counted out of the graded total.
GRADED=$((EXPECTED - 1))

echo "passed=$passed graded=$GRADED control_ok=$control_ok" >> "$LOGS/run.log"

# -------------------------------------------------------------------------
# Generate CTRF report.json
# -------------------------------------------------------------------------

emit_ctrf() {
  local pass="$1"
  local total="$2"
  local fail=$((total - pass))
  cat > "$REPORT" <<CTRF
{
  "results": {
    "tool": {
      "name": "roundel_tests"
    },
    "summary": {
      "tests": $total,
      "passed": $pass,
      "failed": $fail,
      "pending": 0,
      "skipped": 0,
      "other": 0,
      "start": $(date +%s)000,
      "stop": $(date +%s)000
    },
    "tests": []
  }
}
CTRF
}

emit_ctrf "$passed" "$GRADED"

# -------------------------------------------------------------------------
# Reward decision
#
# Written last, and written unconditionally, so that anything the candidate's
# code may have done to reward.txt while the tests ran is overwritten here.
# -------------------------------------------------------------------------

if [ "$passed" -eq "$GRADED" ] && [ "$control_ok" -eq 1 ]; then
  echo "1" > "$REWARD"
  echo "REWARD: 1 (all $GRADED tests passed, control intact)" >> "$LOGS/run.log"
else
  echo "0" > "$REWARD"
  if [ "$control_ok" -ne 1 ]; then
    echo "REWARD: 0 (negative control compromised)" >> "$LOGS/run.log"
  else
    echo "REWARD: 0 ($passed/$GRADED passed)" >> "$LOGS/run.log"
  fi
  if [ "${#failures[@]}" -gt 0 ]; then
    echo "--- failures ---" >> "$LOGS/run.log"
    for n in "${failures[@]}"; do
      echo "  $n" >> "$LOGS/run.log"
    done
  fi
fi

exit 0
