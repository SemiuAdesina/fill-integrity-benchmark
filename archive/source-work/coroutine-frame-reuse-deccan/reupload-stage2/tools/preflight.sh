#!/usr/bin/env bash
# Stage 1 structural preflight for a coroutine-frame-reuse task root.
set -uo pipefail
P="$1"; fails=0
ok(){ printf 'PASS  %s\n' "$1"; }; bad(){ printf 'FAIL  %s\n' "$1"; fails=$((fails+1)); }
for f in task.toml instruction.md environment/Dockerfile environment/CMakeLists.txt tests/test.sh tests/Dockerfile tests/graded.cpp tests/verifier_driver.cpp tests/harness.hpp solution/solve.sh solution/fixed/include/roundel/runtime.hpp solution/fixed/src/runtime.cpp environment/spec/protocol.md; do [ -f "$P/$f" ] || bad "missing $f"; done; ok "required files checked"
grep -q '^schema_version *= *"1.4"' "$P/task.toml" && ok "schema_version 1.4" || bad "schema_version"
grep -q '^name *= *"benchmark-suite/coroutine-frame-reuse"' "$P/task.toml" && ok 'name benchmark-suite/coroutine-frame-reuse' || bad "name"
grep -q '^category *= *"Science"' "$P/task.toml" && ok "category Science" || bad "category"
grep -q '^subcategory *= *"Computer Systems and Runtime Semantics"' "$P/task.toml" && ok "subcategory" || bad "subcategory"
grep -q '^expert_time_estimate_hours *= *3.0' "$P/task.toml" && ok "expert time 3.0" || bad "expert time"
grep -q 'environment_mode *= *"separate"' "$P/task.toml" && ok "separate verifier environment" || bad "environment_mode"
missing=$(find "$P" -type f | while read -r f; do grep -qF "$CANARY" "$f" || echo "$f"; done)
[ -z "$missing" ] && ok "canary on every file" || { bad "canary missing"; echo "$missing"; }
grep -nE '(^|[[:space:]`(])\.{1,2}/' "$P/instruction.md" >/dev/null && { bad "instruction.md has relative path references"; grep -nE '(^|[[:space:]`(])\.{1,2}/' "$P/instruction.md"; } || ok "instruction.md uses absolute paths"
grep -q '/app/' "$P/instruction.md" && ok "instruction.md references /app" || bad "instruction.md lacks /app"
find "$P" -type l | grep -q . && bad "symlinks present" || ok "no symlinks"
find "$P" \( -name .DS_Store -o -name '__MACOSX' -o -name '*.orig' -o -name '*.rej' -o -name build -o -name __pycache__ -o -name '*.o' \) | grep -q . && bad "junk present" || ok "no junk"
for d in environment/Dockerfile tests/Dockerfile; do grep -q '^FROM .*@sha256:' "$P/$d" && ok "$d digest-pinned" || bad "$d not digest-pinned"; grep -qiE '^\s*RUN .*(apt|apk|curl|wget|pip|npm|git clone)' "$P/$d" && bad "$d fetches at build time" || ok "$d no network fetch"; done
ls "$P/environment/diagnostics"/*.cpp 2>/dev/null | wc -l | grep -q '^ *5$' && ok "five diagnostic programs" || bad "diagnostics count"
grep -q 'COPY diagnostics' "$P/environment/Dockerfile" && ok "diagnostics copied into the image" || bad "diagnostics not copied"
n=$(grep -cE '^\{"[a-z0-9_]+","[0-9a-f]{64}",[01],(true|false)\},' "$P/tests/verifier_driver.cpp"); [ "$n" = 75 ] && ok "driver lists 75 cases (74 graded + control)" || bad "driver case count $n"
m=$(grep -c '^ROUNDEL_TEST(' "$P/tests/graded.cpp"); [ "$m" = 75 ] && ok "graded.cpp defines 75 cases" || bad "graded.cpp case count $m"
grep -q 'graded:-74' "$P/tests/test.sh" && ok "test.sh default graded 74" || bad "test.sh graded default"
grep -q 'chmod 700 "\$LOGS"' "$P/tests/test.sh" && grep -q 'rewrote the scenario or the harness' "$P/tests/test.sh" && grep -q 'CANDIDATE_BUILD_TIMEOUT' "$P/tests/test.sh" && ok "test.sh trust-boundary hardening present" || bad "test.sh hardening missing"
grep -q 'verifier link contamination' "$P/tests/test.sh" && ok "judge link inspection present" || bad "link inspection missing"
for w in 'common mistake' 'be careful' "don't use" 'raw address' 'backward' 'this test catches' 'use a stack' 'defer release' 'not a repair' 'not a fix'; do grep -qi -- "$w" "$P/instruction.md" && bad "instruction.md hint phrase: $w"; done; ok "instruction.md hint phrases checked"
echo "preflight failures: $fails"
