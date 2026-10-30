#!/usr/bin/env bash
#
# Installs the reference search and runs it. Everything of substance lives in
# fixed/src/; this script only puts it where the crate expects it and builds.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd /app

# The placeholder solver is one file. The reference is that file plus a module
# directory beside it, so both are installed and any stale copy is replaced.
rm -rf src/solver src/solver.rs
cp "${here}/fixed/src/solver.rs" src/solver.rs
cp -R "${here}/fixed/src/solver" src/solver

cargo build --offline --release
./target/release/plasmid-map
