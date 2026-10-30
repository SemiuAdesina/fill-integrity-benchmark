#!/usr/bin/env bash
# LOCAL STAGE-1 PREFLIGHT for the relocation-infer package (not the platform's own check).
set -uo pipefail
P=~/benchmark-suite-work/relocation-infer-deccan/pkg; ok=1
say() { printf '%-6s %s\n' "$1" "$2"; if [ "$1" = "FAIL" ]; then ok=0; fi; return 0; }
python3 - "$P/task.toml" <<'PY' && say PASS "G02 task.toml parses (tomllib) and has the expected sections" || say FAIL "G02 task.toml does not parse"
import sys, tomllib
d = tomllib.load(open(sys.argv[1], "rb"))
need = {"schema_version", "artifacts", "task", "metadata", "verifier", "agent", "environment"}
assert need <= set(d), set(d)
assert set(d) == need, "extra top-level keys: %s" % (set(d) - need)
m = d["metadata"]
for k in ["author_name", "author_email", "category", "subcategory", "tags", "expert_time_estimate_hours", "relevant_experience", "difficulty_explanation", "solution_explanation", "verification_explanation"]:
    assert k in m, k
assert set(m) <= {"author_name", "author_email", "category", "subcategory", "tags", "expert_time_estimate_hours", "relevant_experience", "difficulty_explanation", "solution_explanation", "verification_explanation"}, set(m)
assert d["task"]["name"] == "benchmark-suite/relocation-infer"
assert d["metadata"]["category"] == "Software" and d["metadata"]["subcategory"] == "Binary Tooling and Linkers"
assert d["metadata"]["expert_time_estimate_hours"] == 4.0
assert d["verifier"]["environment_mode"] == "separate"
assert "PLACEHOLDER" not in m["difficulty_explanation"] + m["solution_explanation"] + m["verification_explanation"]
PY
grep -q 'schema_version = "1.4"' "$P/task.toml" && say PASS "G02 schema_version 1.4 (latest platform-passing package)" || say FAIL "G02 schema_version"
grep -qE '^/app|`/app' "$P/instruction.md" && ! grep -qE '(^|[^/a-zA-Z])\./(src|spec|corpus)' "$P/instruction.md" && say PASS "G01 instruction uses absolute /app paths" || say FAIL "G01 relative path in instruction"
for f in $(grep -oE '/app/[A-Za-z0-9_./-]+' "$P/instruction.md" | sort -u); do
  rel="${f#/app/}"; rel="${rel%%[[:space:]]*}"
  case "$rel" in relocations.json|target/*) continue;; esac
  [ -e "$P/environment/$rel" ] || say FAIL "G01 instruction references missing path $f"
done
say PASS "G01 every /app path in the instruction exists in the environment"
grep -qE '^FROM rust@sha256:' "$P/environment/Dockerfile" && grep -qE '^FROM rust@sha256:' "$P/tests/Dockerfile" && say PASS "G04 both images digest-pinned" || say FAIL "G04 image not digest-pinned"
! grep -qE 'apt-get|apt |curl |wget |pip3? install|cargo install|rustup (install|toolchain|update|default)|git clone' "$P/environment/Dockerfile" "$P/tests/Dockerfile" "$P/tests/test.sh" && say PASS "G04 no package fetch / network step" || say FAIL "G04 network/package step present"
grep -q 'CARGO_NET_OFFLINE=true' "$P/environment/Dockerfile" && say PASS "G04 offline cargo" || say FAIL "G04 cargo not offline"
deps=$(awk '/^\[dependencies\]/{f=1;next} /^\[/{f=0} f && /=/' "$P/environment/Cargo.toml" "$P/tests/verifier/Cargo.toml" | wc -l | tr -d ' '); [ "$deps" -eq 0 ] && say PASS "G04 no third-party dependencies" || say FAIL "G04 dependencies present"
[ -z "$(find "$P" -type l)" ] && say PASS "G04 no symlinks in package" || say FAIL "G04 symlink in package"
[ -z "$(find "$P" \( -name .DS_Store -o -name '*.orig' -o -name '__MACOSX' -o -name '*.pyc' \))" ] && say PASS "G04 no junk files" || say FAIL "G04 junk files"
[ -z "$(find "$P" -type d -name target)" ] && say PASS "G04 no build artifacts" || say FAIL "G04 target/ directory present"
[ -d "$P/environment" ] && [ -d "$P/tests" ] && [ -d "$P/solution" ] && [ -f "$P/task.toml" ] && [ -f "$P/instruction.md" ] && say PASS "G05 canonical layout environment/ tests/ solution/ task.toml instruction.md" || say FAIL "G05 layout"
t=$(find "$P" -type f \( -name '*.md' -o -name '*.rs' -o -name '*.toml' -o -name '*.sh' -o -name 'Dockerfile' -o -name '.dockerignore' \) | wc -l | tr -d ' '); n=$(find "$P" -type f \( -name '*.md' -o -name '*.rs' -o -name '*.toml' -o -name '*.sh' -o -name 'Dockerfile' -o -name '.dockerignore' \) -exec grep -l -i canary {} + | wc -l | tr -d ' ')
[ "$n" -eq "$t" ] && say PASS "G03 canary present in every text file ($n/$t)" || say FAIL "G03 canary present in $n of $t text files"
v=$(grep -rhoi 'canary[^0-9a-f]*[0-9a-f-]\{36\}' "$P" | grep -oE '[0-9a-f-]{36}' | sort -u | wc -l | tr -d ' '); [ "$v" -eq 1 ] && say PASS "G03 exactly one canary value" || say FAIL "G03 canary values: $v"
for word in "Stage 2" "Stage 3" rollout "2/8" Claude benchmark "pass rate" "difficulty gate" "model rollout" latent oracle_a oracle_b; do
  if grep -rqi -- "$word" "$P/environment" "$P/instruction.md" 2>/dev/null; then say FAIL "leakage: '$word' in candidate-visible files"; fi
done
say PASS "candidate-visible leakage scan (terms) done"
find "$P/environment" -name '*expected*' | grep -q . && say FAIL "expected-answer file in environment" || say PASS "no expected-answer file in environment"
grep -qE 'EXPECTED_JOBS=8' "$P/tests/test.sh" && [ "$(grep -cE '^\s*job!\("' "$P/tests/verifier/src/plan.rs")" -eq 8 ] && say PASS "job count 8 matches plan" || say FAIL "job count"
grep -qE 'EXPECTED_CHECKS=13' "$P/tests/test.sh" && [ "$(grep -cE '^\s*check!\(' "$P/tests/verifier/src/bin/judge/main.rs")" -eq 13 ] && say PASS "check count 13 matches judge" || say FAIL "check count"
for m in json elf corpus program bits; do cmp -s "$P/environment/src/$m.rs" "$P/tests/verifier/src/$m.rs" || say FAIL "frozen module $m differs between environment and verifier"; done; say PASS "frozen modules identical in both crates"
cmp -s "$P/environment/corpus/manifest.json" "$P/tests/fixtures/public/corpus/manifest.json" && say PASS "public corpus identical in environment and fixtures" || say FAIL "public corpus differs"
w=$(wc -w < "$P/instruction.md" | tr -d ' '); [ "$w" -le 900 ] && say PASS "instruction.md $w words" || say FAIL "instruction.md too long ($w words)"

grep -qE '^timeout_sec = 28800' <(sed -n '/^\[agent\]/,/^\[/p' "$P/task.toml") && say PASS "TB4 [agent] timeout_sec = 28800" || say FAIL "TB4 [agent] timeout_sec must be 28800"
CLOSE='You have 28800 seconds to complete this task. Do not cheat by using online solutions or hints specific to this task.'
[ "$(tail -1 "$P/instruction.md")" = "$CLOSE" ] && [ -z "$(tail -2 "$P/instruction.md" | head -1)" ] && say PASS "TB4 instruction ends with a blank line and the mandated sentence" || say FAIL "TB4 instruction closing sentence missing"
for f in task.toml instruction.md environment/Dockerfile solution/solve.sh tests/Dockerfile tests/test.sh; do head -3 "$P/$f" | grep -qi canary || say FAIL "canary not within the first 3 lines of core file $f"; done; say PASS "canary within the first 3 lines of the six core files"


! grep -qE 'chmod [0-7]+ "\$REWARD"|chmod [0-7]+ "\$LOGS/reward' "$P/tests/test.sh" && say PASS "reward file keeps the default mode (host collector is not root)" || say FAIL "test.sh restricts the reward file mode; Harbor host cannot read it"

[ $ok -eq 1 ] && echo "LOCAL STAGE-1 PREFLIGHT PASS" || echo "LOCAL STAGE-1 PREFLIGHT FAIL"
