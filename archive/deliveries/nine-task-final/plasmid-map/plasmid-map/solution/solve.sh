#!/usr/bin/env bash
set -euo pipefail
APP_DIR="${APP_DIR:-/app}"; HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")"&&pwd)"
test -f "$APP_DIR/Cargo.toml"; test ! -L "$APP_DIR/src/lib.rs"
cp "$HERE/fixed/src/lib.rs" "$APP_DIR/src/lib.rs"; cd "$APP_DIR"; cargo build --offline
