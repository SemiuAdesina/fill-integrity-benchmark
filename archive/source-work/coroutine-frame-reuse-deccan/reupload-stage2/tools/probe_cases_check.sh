#!/usr/bin/env bash
# Build one candidate (runtime.hpp/runtime.cpp dir) against a scenario file inside
# the task image, run every case the way the driver does, print "<case> <rc> <sha256>".
#   probe_cases.sh <scenario.cpp> <candidate_dir> <out_file>
set -uo pipefail
SCEN="$1"; CAND="$2"; OUTF="$3"
W=~/benchmark-suite-work/coroutine-frame-reuse-deccan/reupload-stage2
docker run --rm --network none \
  -v "$W/pkg/tests/frozen:/frozen:ro" -v "$W/lab/checkharness/harness.hpp:/h/harness.hpp:ro" \
  -v "$SCEN:/s/scenario.cpp:ro" -v "$CAND:/cand:ro" rc-agent:reup-base /bin/bash -c '
set -e
mkdir -p /b/include/roundel && cp /cand/runtime.hpp /b/include/roundel/ && cp /cand/runtime.cpp /b/
CXX=$(command -v clang++); F="-std=c++20 -g -fsanitize=address,undefined -fno-omit-frame-pointer -I/frozen/include -I/b/include -I/h"
$CXX $F -Wall -Wextra -Wpedantic -Werror -Wno-unused-but-set-variable -Wno-unused-const-variable -c /b/runtime.cpp -o /b/runtime.o
$CXX $F -w -c /s/scenario.cpp -o /b/scenario.o
$CXX $F -w -c /frozen/src/arena.cpp -o /b/arena.o
$CXX $F -w -c /frozen/src/journal.cpp -o /b/journal.o
$CXX $F -o /b/cand /b/scenario.o /b/arena.o /b/journal.o /b/runtime.o
for t in $(/b/cand --list); do
  out=$(cd /tmp && env -i HOME=/tmp/roundel-candidate-home PATH=/usr/bin:/bin \
     ASAN_OPTIONS=detect_leaks=1:abort_on_error=0:allocator_may_return_null=0:detect_stack_use_after_return=1 \
     UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1 LSAN_OPTIONS=report_objects=1 \
     timeout 10 /b/cand --run $t 2>&1; echo "RC=$?")
  rc=$(echo "$out" | tail -1 | sed "s/RC=//"); body=$(echo "$out" | sed "\$d")
  sha=$(printf "%s\n" "$body" | sha256sum | cut -d" " -f1)
  echo "$t $rc $sha"
  printf "%s\n" "$body" > "/tmp/obs.$t.txt"
done
tar -C /tmp -cf - $(ls /tmp | grep "^obs\.") | base64 -w0 > /tmp/obs.b64; echo "OBSTAR $(cat /tmp/obs.b64)"
' > "$OUTF" 2>&1
grep -v '^OBSTAR' "$OUTF" | grep -E '^[a-z0-9_]+ [0-9]+ [0-9a-f]{64}$' > "$OUTF.table"
mkdir -p "$OUTF.obs"; grep '^OBSTAR' "$OUTF" | sed 's/^OBSTAR //' | base64 -d | tar -C "$OUTF.obs" -xf - 2>/dev/null
echo "$(basename "$CAND") cases=$(wc -l < "$OUTF.table")"
