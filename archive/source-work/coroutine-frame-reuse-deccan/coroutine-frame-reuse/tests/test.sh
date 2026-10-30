#!/usr/bin/env bash
# Verifier-owned out-of-process grading runner.

set -uo pipefail

unset BASH_ENV ENV CDPATH GLOBIGNORE SHELLOPTS 2>/dev/null || true
unset CC CXX AR LD AS RANLIB STRIP OBJCOPY OBJDUMP NM READELF
unset CFLAGS CXXFLAGS CPPFLAGS LDFLAGS LDLIBS
unset CMAKE_C_COMPILER CMAKE_CXX_COMPILER CMAKE_AR CMAKE_LINKER
unset CMAKE_C_FLAGS CMAKE_CXX_FLAGS CMAKE_EXE_LINKER_FLAGS CMAKE_SHARED_LINKER_FLAGS
unset CMAKE_MODULE_LINKER_FLAGS CMAKE_STATIC_LINKER_FLAGS CMAKE_PREFIX_PATH
unset CMAKE_TOOLCHAIN_FILE CMAKE_GENERATOR CMAKE_MAKE_PROGRAM
unset LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH
unset ASAN_OPTIONS UBSAN_OPTIONS LSAN_OPTIONS MSAN_OPTIONS TSAN_OPTIONS
unset CPATH CPLUS_INCLUDE_PATH C_INCLUDE_PATH LIBRARY_PATH COMPILER_PATH

export HOME=/tmp/roundel-verifier-home
export PATH=/usr/local/bin:/usr/bin:/bin

LOGS=/logs/verifier
REWARD="$LOGS/reward.txt"
REPORT="$LOGS/report.json"
SUMMARY="$LOGS/driver-summary.txt"

/bin/mkdir -p "$LOGS" "$HOME"
/usr/bin/printf '0\n' > "$REWARD"

fail_build() {
  /usr/bin/printf '%s\n' "$1" >&2
  /usr/bin/printf '0\n' > "$REWARD"
  exit 0
}

CLANG=/usr/local/bin/clang
CLANGXX=/usr/local/bin/clang++
CMAKE=/usr/local/bin/cmake
[ -x "$CLANG" ] || CLANG=/usr/bin/clang
[ -x "$CLANGXX" ] || CLANGXX=/usr/bin/clang++
[ -x "$CMAKE" ] || CMAKE=/usr/bin/cmake
[ -x "$CLANG" ] && [ -x "$CLANGXX" ] && [ -x "$CMAKE" ] || fail_build "trusted toolchain missing"

APP=/app
CANDIDATE_HPP="$APP/include/roundel/runtime.hpp"
CANDIDATE_CPP="$APP/src/runtime.cpp"
[ -f "$CANDIDATE_HPP" ] || fail_build "candidate runtime.hpp missing"
[ -f "$CANDIDATE_CPP" ] || fail_build "candidate runtime.cpp missing"
[ ! -L "$CANDIDATE_HPP" ] || fail_build "candidate runtime.hpp is a symlink"
[ ! -L "$CANDIDATE_CPP" ] || fail_build "candidate runtime.cpp is a symlink"

# Build submitted code as uid/gid 65534 from an expected-free staging tree.
# The verifier driver and its expected transcript digests are not readable to
# this compiler or to the process later being graded.
CAND_SRC=/candidate-src
CAND_BUILD=/candidate-build
VERIFIER_BUILD=/verifier-build
BIN_DIR=/verifier-bin
/bin/rm -rf "$CAND_SRC" "$CAND_BUILD" "$VERIFIER_BUILD" "$BIN_DIR"
/bin/mkdir -p "$CAND_SRC/frozen/include/roundel" "$CAND_SRC/frozen/src" \
  "$CAND_SRC/candidate/include/roundel" "$CAND_SRC/candidate/src" \
  "$CAND_BUILD" "$VERIFIER_BUILD" "$BIN_DIR"

/bin/cp /tests/CMakeLists.txt /tests/harness.hpp "$CAND_SRC/"
/bin/cp /tests/frozen/include/roundel/arena.hpp /tests/frozen/include/roundel/journal.hpp "$CAND_SRC/frozen/include/roundel/"
/bin/cp /tests/frozen/src/arena.cpp /tests/frozen/src/journal.cpp "$CAND_SRC/frozen/src/"
/bin/cp "$CANDIDATE_HPP" "$CAND_SRC/candidate/include/roundel/runtime.hpp"
/bin/cp "$CANDIDATE_CPP" "$CAND_SRC/candidate/src/runtime.cpp"
/bin/chown -R 65534:65534 "$CAND_SRC" "$CAND_BUILD"
/bin/chmod 700 /tests

RUN_UNPRIV=()
if [ -x /usr/sbin/runuser ]; then
  RUN_UNPRIV=(/usr/sbin/runuser -u nobody --)
elif [ -x /usr/bin/setpriv ]; then
  RUN_UNPRIV=(/usr/bin/setpriv --reuid=65534 --regid=65534 --clear-groups)
else
  fail_build "no trusted privilege-drop utility"
fi

# Feed the root-owned scenario definitions to the unprivileged preprocessor on
# stdin. CHECK_EQ/CHECK_LE/CHECK_LT discard their verifier-only expected operand,
# leaving scenario.cpp with actual observations only. The preprocessor cannot
# open /tests or the expected-digest driver, and the raw graded.cpp is never
# copied into its readable staging tree.
if ! {
  cd /tmp || exit 1
  /bin/cat /tests/graded.cpp | "${RUN_UNPRIV[@]}" /usr/bin/env -i \
    HOME=/tmp PATH=/usr/local/bin:/usr/bin:/bin \
    "$CLANGXX" -std=c++20 -E -P -x c++ \
      -I"$CAND_SRC/frozen/include" -I"$CAND_SRC/candidate/include" -I"$CAND_SRC" \
      -o "$CAND_SRC/scenario.cpp" -
} > "$LOGS/candidate-preprocess.log" 2>&1; then
  fail_build "candidate scenario preprocessing failed"
fi

{
  /usr/bin/printf '%s\n' '=== candidate configure/build (uid 65534) ==='
  "${RUN_UNPRIV[@]}" /usr/bin/env -i HOME=/tmp PATH=/usr/local/bin:/usr/bin:/bin \
    "$CMAKE" -S "$CAND_SRC" -B "$CAND_BUILD" -DBUILD_CANDIDATE=ON \
      -DCMAKE_BUILD_TYPE=Debug -DCMAKE_C_COMPILER="$CLANG" -DCMAKE_CXX_COMPILER="$CLANGXX"
  "${RUN_UNPRIV[@]}" /usr/bin/env -i HOME=/tmp PATH=/usr/local/bin:/usr/bin:/bin \
    "$CMAKE" --build "$CAND_BUILD" --target roundel_candidate -j 4
} > "$LOGS/candidate-build.log" 2>&1 || fail_build "candidate build failed"

[ -x "$CAND_BUILD/roundel_candidate" ] || fail_build "candidate executable missing"
/bin/cp "$CAND_BUILD/roundel_candidate" "$BIN_DIR/roundel_candidate"
/bin/chown root:root "$BIN_DIR/roundel_candidate"
/bin/chmod 755 "$BIN_DIR/roundel_candidate" "$BIN_DIR"
CAND_LINK=$(/usr/bin/find "$CAND_BUILD" -path '*/roundel_candidate.dir/link.txt' -type f -print -quit)
[ -n "$CAND_LINK" ] || fail_build "candidate link command missing"
/bin/cp "$CAND_LINK" "$LOGS/candidate-link.txt"

# Erase readable build inputs before candidate execution.
/bin/rm -rf "$CAND_SRC" "$CAND_BUILD"

{
  /usr/bin/printf '%s\n' '=== verifier configure/build (no candidate target) ==='
  "$CMAKE" -S /tests -B "$VERIFIER_BUILD" -DBUILD_CANDIDATE=OFF \
    -DCMAKE_BUILD_TYPE=Release -DCMAKE_C_COMPILER="$CLANG" -DCMAKE_CXX_COMPILER="$CLANGXX"
  "$CMAKE" --build "$VERIFIER_BUILD" --target roundel_verifier -j 4
} > "$LOGS/verifier-build.log" 2>&1 || fail_build "verifier build failed"

[ -x "$VERIFIER_BUILD/roundel_verifier" ] || fail_build "verifier executable missing"
/bin/cp "$VERIFIER_BUILD/roundel_verifier" "$BIN_DIR/roundel_verifier"
/bin/chown root:root "$BIN_DIR/roundel_verifier"
/bin/chmod 700 "$BIN_DIR/roundel_verifier"

# Preserve exact link evidence. This line must contain only verifier_driver.cpp.o.
LINK_FILE=$(/usr/bin/find "$VERIFIER_BUILD" -path '*/roundel_verifier.dir/link.txt' -type f -print -quit)
[ -n "$LINK_FILE" ] || fail_build "verifier link command missing"
/bin/cp "$LINK_FILE" "$LOGS/verifier-link.txt"
if /usr/bin/grep -Eq 'runtime\.cpp|roundel_candidate|roundel_runtime|journal\.cpp|arena\.cpp' "$LOGS/verifier-link.txt"; then
  fail_build "verifier link contamination detected"
fi
/bin/rm -rf "$VERIFIER_BUILD"

driver_rc=0
"$BIN_DIR/roundel_verifier" "$BIN_DIR/roundel_candidate" "$SUMMARY" > "$LOGS/driver.log" 2>&1 || driver_rc=$?

passed=$(/usr/bin/sed -n 's/^passed=//p' "$SUMMARY" 2>/dev/null)
graded=$(/usr/bin/sed -n 's/^graded=//p' "$SUMMARY" 2>/dev/null)
control_ok=$(/usr/bin/sed -n 's/^control_ok=//p' "$SUMMARY" 2>/dev/null)
passed=${passed:-0}; graded=${graded:-70}; control_ok=${control_ok:-0}

/usr/bin/printf '{\n  "results": {\n    "tool": {"name": "roundel_out_of_process"},\n    "summary": {"tests": %s, "passed": %s, "failed": %s, "pending": 0, "skipped": 0, "other": 0, "start": 0, "stop": 0},\n    "tests": []\n  }\n}\n' \
  "$graded" "$passed" "$((graded-passed))" > "$REPORT"

if [ "$driver_rc" -eq 0 ] && [ "$passed" -eq "$graded" ] && [ "$control_ok" -eq 1 ]; then
  /usr/bin/printf '1\n' > "$REWARD"
else
  /usr/bin/printf '0\n' > "$REWARD"
fi

exit 0
