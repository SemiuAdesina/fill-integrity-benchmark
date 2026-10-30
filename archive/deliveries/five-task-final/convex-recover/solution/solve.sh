#!/usr/bin/env bash
set -euo pipefail
A="${APP_DIR:-/app}";H="$(cd "$(dirname "${BASH_SOURCE[0]}")"&&pwd)";test -f "$A/Cargo.toml";test ! -L "$A/src/lib.rs";cp "$H/fixed/src/lib.rs" "$A/src/lib.rs";cd "$A";cargo build --offline
