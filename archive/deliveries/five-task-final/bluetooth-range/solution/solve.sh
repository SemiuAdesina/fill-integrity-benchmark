#!/usr/bin/env bash
set -euo pipefail
APP_DIR="${APP_DIR:-/app}"
SOLUTION_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FIXED="${SOLUTION_DIR}/fixed/src/lib.rs"
[ -f "${APP_DIR}/Cargo.toml" ] || { echo "missing /app crate" >&2; exit 1; }
[ -f "${FIXED}" ] || { echo "missing fixed source" >&2; exit 1; }
cp "${FIXED}" "${APP_DIR}/src/lib.rs"
export CARGO_NET_OFFLINE=true
cd "${APP_DIR}"
cargo fmt -- src/lib.rs >/dev/null 2>&1 || true
cargo build --offline
