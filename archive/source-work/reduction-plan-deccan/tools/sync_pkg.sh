#!/usr/bin/env bash
set -euo pipefail
W=~/benchmark-suite-work/reduction-plan-deccan; P=$W/pkg
mkdir -p "$P/tests/verifier/include/fabric" "$P/tests/verifier/src"
cp "$P/environment/include/fabric/json.hpp" "$P/environment/include/fabric/model.hpp" "$P/tests/verifier/include/fabric/"
cp "$P/environment/src/json.cpp" "$P/environment/src/model.cpp" "$P/tests/verifier/src/"
rm -rf "$P/tests/fixtures"; mkdir -p "$P/tests/fixtures"
for f in $(ls "$W/author/fixtures"); do
  case "$f" in x_*) continue;; esac   # x_* fixtures are public examples, never graded
  mkdir -p "$P/tests/fixtures/$f"; cp "$W/author/fixtures/$f/problem.json" "$P/tests/fixtures/$f/"; cp "$W/author/fixtures/$f/expected.json" "$P/tests/fixtures/$f.expected.json"; done
cp "$W/author/fixtures/public/problem.json" "$P/environment/problem.json"
if [ -d "$W/author/fixtures/x_scale_sample" ]; then
  mkdir -p "$P/environment/examples/scale_sample"
  cp "$W/author/fixtures/x_scale_sample/problem.json" "$P/environment/examples/scale_sample/problem.json"
  cp "$W/author/fixtures/x_scale_sample/result.expected.json" "$P/environment/examples/scale_sample/result.json"
fi
find "$P" -name .DS_Store -delete
echo "synced: $(ls "$W/author/fixtures" | wc -l | tr -d ' ') fixtures"
