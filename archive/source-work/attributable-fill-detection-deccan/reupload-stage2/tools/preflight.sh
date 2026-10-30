#!/usr/bin/env bash
# Stage 1 structural preflight for an attributable-fill-detection task root.
set -uo pipefail
P="$1"; fails=0
ok(){ printf 'PASS  %s\n' "$1"; }; bad(){ printf 'FAIL  %s\n' "$1"; fails=$((fails+1)); }
for f in task.toml instruction.md environment/Dockerfile environment/Cargo.toml environment/Cargo.lock tests/test.sh tests/judge.sh tests/Dockerfile tests/graded.rs tests/frozen_probe.rs tests/common/mod.rs tests/cases.txt tests/Cargo.toml tests/Cargo.lock solution/solve.sh solution/fixed/src/engine/cycle.rs solution/fixed/src/engine/fill_classifier.rs solution/fixed/src/types/snapshot.rs environment/spec/venue-semantics.md environment/spec/reconciliation-contract.md environment/spec/frozen-contract.md environment/spec/source-map.md environment/tests/harness/mod.rs; do [ -f "$P/$f" ] || bad "missing $f"; done; ok "required files checked"
grep -q '^schema_version *= *"1.4"' "$P/task.toml" && ok "schema_version 1.4" || bad "schema_version"
grep -q '^name *= *"benchmark-suite/attributable-fill-detection"' "$P/task.toml" && ok 'name benchmark-suite/attributable-fill-detection' || bad "name"
grep -q '^category *= *"Software"' "$P/task.toml" && ok "category Software" || bad "category"
grep -q '^subcategory *= *"Systems"' "$P/task.toml" && ok "subcategory Systems" || bad "subcategory"
grep -q '^expert_time_estimate_hours *= *3.0' "$P/task.toml" && ok "expert time 3.0" || bad "expert time"
grep -q 'environment_mode *= *"separate"' "$P/task.toml" && ok "separate verifier environment" || bad "environment_mode"
grep -q 'allow_internet' "$P/task.toml" && bad "deprecated allow_internet present" || ok "no allow_internet"
python3 -c "import tomllib,sys; tomllib.load(open('$P/task.toml','rb'))" 2>/dev/null && ok "task.toml parses" || bad "task.toml does not parse"
missing=$(find "$P" -type f ! -path '*/tests/expected/*' ! -name '*.lock' | while read -r f; do grep -qF "$CANARY" "$f" || echo "$f"; done)
[ -z "$missing" ] && ok "canary on every non-record file" || { bad "canary missing"; echo "$missing"; }
grep -rl '8ebdff1834d9ad3a406473bb86979e6a' "$P" | grep -q . && bad "old canary still present" || ok "old canary absent"
grep -nE '(^|[[:space:]`(])\.{1,2}/' "$P/instruction.md" >/dev/null && { bad "instruction.md has relative path references"; grep -nE '(^|[[:space:]`(])\.{1,2}/' "$P/instruction.md"; } || ok "instruction.md uses absolute paths"
grep -q '/app/' "$P/instruction.md" && ok "instruction.md references /app" || bad "instruction.md lacks /app"
find "$P" -type l | grep -q . && bad "symlinks present" || ok "no symlinks"
find "$P" \( -name .DS_Store -o -name '__MACOSX' -o -name '*.orig' -o -name '*.rej' -o -name target -o -name __pycache__ \) | grep -q . && bad "junk present" || ok "no junk"
for d in environment/Dockerfile tests/Dockerfile; do grep -q '^FROM .*@sha256:' "$P/$d" && ok "$d digest-pinned" || bad "$d not digest-pinned"; grep -qiE '^\s*RUN .*(apt|apk|curl|wget|pip|npm|git clone)' "$P/$d" && bad "$d fetches at build time" || ok "$d no network fetch"; done
n=$(grep -cE '^(graded|frozen) ' "$P/tests/cases.txt"); [ "$n" = 53 ] && ok "cases.txt lists 53 cases" || bad "cases.txt count $n"
e=$(ls "$P/tests/expected" | wc -l | tr -d ' '); [ "$e" = 53 ] && ok "53 expected records" || bad "expected count $e"
for c in $(awk '/^(graded|frozen) /{print $2}' "$P/tests/cases.txt"); do [ -f "$P/tests/expected/$c" ] || bad "no expected record for $c"; done
g=$(grep -c '^#\[test\]' "$P/tests/graded.rs"); [ "$g" = 37 ] && ok "graded.rs defines 37 tests" || bad "graded.rs test count $g"
grep -q '"$total" -eq 53' "$P/tests/test.sh" && [ "$(grep -c '\-eq 53' "$P/tests/judge.sh")" = 2 ] && ok "test.sh/judge.sh expect 53" || bad "verifier counts"
grep -q 'chmod 700 "\$LOGS"' "$P/tests/test.sh" && grep -q 'reward reset to 0 after the candidate phase' "$P/tests/test.sh" && grep -q '^BUILD_TIMEOUT=' "$P/tests/test.sh" && grep -q '^CASE_TIMEOUT=' "$P/tests/test.sh" && grep -q '^PHASE_BUDGET=' "$P/tests/test.sh" && ok "test.sh trust-boundary and budget hardening present" || bad "test.sh hardening missing"
ls "$P/environment/tests"/*.rs | wc -l | grep -q '^ *6$' && ok "six diagnostic suites" || bad "diagnostic suite count"
[ -e "$P/environment/src/bin" ] && bad "src/bin still present" || ok "no reproducer binary"
[ -e "$P/environment/tests/diagnostics.rs" ] && bad "old diagnostics.rs still present" || ok "old rule-naming diagnostics removed"
for w in 'What has to be true' 'high-water' 'common mistake' 'be careful' 'the defects are' 'structure is sound' 'not in how the state is stored' 'source-map.md identifies' 'this test catches'; do grep -qi -- "$w" "$P/instruction.md" && bad "instruction.md hint phrase: $w"; done; ok "instruction.md hint phrases checked"
for w in 'BATTERY_PARAGRAPH' 'Stage 2' 'Stage 3' 'HY4' 'hy4' 'Claude' 'turn count' 'rollout' 'benchmark' 'difficulty gate' '2/8' '6/8'; do grep -rqi --include='*.md' --include='*.rs' --include='*.toml' --include='*.sh' -- "$w" "$P" && bad "leak phrase in package: $w"; done; ok "package leakage phrases checked"
wc -w "$P/instruction.md" | awk '{print "      instruction.md words:", $1}'
echo "preflight failures: $fails"
