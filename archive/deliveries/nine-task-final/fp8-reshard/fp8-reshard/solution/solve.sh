#!/usr/bin/env bash
set -euo pipefail
APP_DIR="${APP_DIR:-/app}"; HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")"&&pwd)"
test -f "$HERE/fixed/src/restore.cpp"; test -f "$APP_DIR/src/restore.cpp"
cp "$HERE/fixed/src/restore.cpp" "$APP_DIR/src/restore.cpp"
cmake -S "$APP_DIR" -B "$APP_DIR/build" -DCMAKE_BUILD_TYPE=Debug >/dev/null
cmake --build "$APP_DIR/build" -j 2
