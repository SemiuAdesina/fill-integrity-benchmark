#!/usr/bin/env bash
#
# Installs the reference recovery and runs it. Everything of substance lives in
# fixed/src/; this script only puts it where the crate expects it and builds.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

cd /app

# The placeholder is one file. The reference is that file plus a module
# directory beside it, so both are installed and any stale copy is replaced.
rm -rf src/recover src/recover.rs
cp "${here}/fixed/src/recover.rs" src/recover.rs
cp -R "${here}/fixed/src/recover" src/recover

cargo build --offline --release
./target/release/reloc-recover
