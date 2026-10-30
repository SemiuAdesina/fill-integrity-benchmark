#!/usr/bin/env bash
set -euo pipefail
APP_DIR="${APP_DIR:-/app}"
SOLUTION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
test -f "$APP_DIR/Cargo.toml"
test -f "$SOLUTION_DIR/fixed/src/lib.rs"
cp "$SOLUTION_DIR/fixed/src/lib.rs" "$APP_DIR/src/lib.rs"
cd "$APP_DIR"
cargo build --offline
