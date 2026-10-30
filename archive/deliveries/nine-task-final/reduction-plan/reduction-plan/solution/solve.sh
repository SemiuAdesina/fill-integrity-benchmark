#!/usr/bin/env bash
set -euo pipefail
APP_DIR="${APP_DIR:-/app}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "${APP_DIR}/CMakeLists.txt" ] || { echo 'missing /app project' >&2; exit 1; }
[ -f "${HERE}/fixed/src/recover.cpp" ] || { echo 'missing fixed source' >&2; exit 1; }
cp "${HERE}/fixed/src/recover.cpp" "${APP_DIR}/src/recover.cpp"
cmake -S "${APP_DIR}" -B "${APP_DIR}/build" -DCMAKE_BUILD_TYPE=Debug
cmake --build "${APP_DIR}/build" -j "$(nproc 2>/dev/null || echo 2)"
