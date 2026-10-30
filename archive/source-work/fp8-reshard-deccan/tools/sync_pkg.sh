#!/usr/bin/env bash
# Copies the frozen sources into the verifier tree, the certified fixtures into tests/fixtures,
# and the public inputs into environment/. Idempotent.
set -euo pipefail
W=~/benchmark-suite-work/fp8-reshard-deccan; P=$W/pkg
mkdir -p "$P/tests/verifier/include/fp8" "$P/tests/verifier/src"
cp "$P/environment/include/fp8/json.hpp" "$P/environment/include/fp8/model.hpp" "$P/tests/verifier/include/fp8/"
cp "$P/environment/src/json.cpp" "$P/environment/src/model.cpp" "$P/tests/verifier/src/"
rm -rf "$P/tests/fixtures"; mkdir -p "$P/tests/fixtures"
for f in $(ls "$W/author/fixtures"); do
  mkdir -p "$P/tests/fixtures/$f"
  cp "$W/author/fixtures/$f/checkpoint.json" "$W/author/fixtures/$f/target.json" "$P/tests/fixtures/$f/"
  cp "$W/author/fixtures/$f/expected.json" "$P/tests/fixtures/$f.expected.json"
done
cp "$W/author/fixtures/public/checkpoint.json" "$P/environment/checkpoint.json"
cp "$W/author/fixtures/public/target.json" "$P/environment/target.json"
find "$P" -name .DS_Store -delete
echo "synced: $(ls "$W/author/fixtures" | wc -l | tr -d ' ') fixtures"
