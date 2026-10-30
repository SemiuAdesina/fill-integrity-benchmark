#!/usr/bin/env bash
# Copies the frozen model into the verifier crate, the certified fixtures into
# tests/fixtures, and the public problem into environment/. Truth files never move.
set -euo pipefail
W=~/benchmark-suite-work/plasmid-map-deccan; P=$W/pkg
for f in json fasta input sites digest map_model result_output; do cp "$P/environment/src/$f.rs" "$P/tests/verifier/src/$f.rs"; done
cp "$P/environment/src/solver.rs" "$P/tests/verifier/src/bin/probe/solver.rs"
rm -rf "$P/tests/fixtures"; mkdir -p "$P/tests/fixtures"
for f in $(ls "$W/author/fixtures"); do
  mkdir -p "$P/tests/fixtures/$f"
  for name in backbone.fasta enzymes.json digests.json; do cp "$W/author/fixtures/$f/$name" "$P/tests/fixtures/$f/"; done
  cp "$W/author/fixtures/$f/expected.json" "$P/tests/fixtures/$f.expected.json"
done
for name in backbone.fasta enzymes.json digests.json; do cp "$W/author/fixtures/public/$name" "$P/environment/$name"; done
find "$P" -name .DS_Store -delete
if find "$P" -name 'truth.author.json' -o -name 'certification.json' | grep -q .; then echo "AUTHOR FILE LEAKED"; exit 1; fi
echo "synced: $(ls "$W/author/fixtures" | wc -l | tr -d ' ') fixtures"
