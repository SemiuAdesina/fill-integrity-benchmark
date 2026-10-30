#!/usr/bin/env bash
# Copies the frozen modules into the verifier crate, the certified fixtures into
# tests/fixtures, and the public corpus into environment/corpus. Idempotent.
set -euo pipefail
W=~/benchmark-suite-work/relocation-infer-deccan
P=$W/pkg
for m in json elf corpus program bits; do cp "$P/environment/src/$m.rs" "$P/tests/verifier/src/$m.rs"; done
# the checked-in probe carries the starter's recover so the verifier crate builds standalone
cp "$P/environment/src/recover.rs" "$P/tests/verifier/src/bin/probe/recover.rs"
rm -rf "$P/tests/verifier/src/bin/probe/recover"
rm -rf "$P/tests/fixtures"; mkdir -p "$P/tests/fixtures"
for f in public case_a case_b case_c case_d case_e case_f; do
  mkdir -p "$P/tests/fixtures/$f"
  cp -R "$W/author/fixtures/$f/corpus" "$P/tests/fixtures/$f/corpus"
  cp "$W/author/fixtures/$f/expected.json" "$P/tests/fixtures/$f.expected.json"
done
rm -rf "$P/environment/corpus"; cp -R "$W/author/fixtures/public/corpus" "$P/environment/corpus"
find "$P" -name .DS_Store -delete
echo "synced: $(find "$P/tests/fixtures" -type f | wc -l | tr -d ' ') fixture files, public corpus $(find "$P/environment/corpus" -type f | wc -l | tr -d ' ') files"
