#!/usr/bin/env bash
# Builds the re-upload ZIP from the worktree and verifies the archive shape.
set -euo pipefail
R=~/benchmark-suite-work/attributable-fill-detection-deccan/reupload-stage2
OUT="$1"
STAGE="$(mktemp -d /tmp/pkgstage-XXXXXX)"
cp -R "$R/pkg" "$STAGE/attributable-fill-detection"
find "$STAGE" \( -name '.DS_Store' -o -name '*.orig' -o -name '*.rej' -o -name '__MACOSX' -o -name '__pycache__' -o -name target \) -print -exec rm -rf {} + 2>/dev/null || true
if find "$STAGE" -type l -print -quit | grep -q .; then echo "SYMLINK PRESENT -- refusing"; exit 1; fi
rm -f "$OUT"
( cd "$STAGE" && zip -q -r -X "$OUT" attributable-fill-detection )
echo "path:    $OUT"
echo "bytes:   $(stat -f%z "$OUT")"
echo "sha256:  $(shasum -a 256 "$OUT" | cut -d' ' -f1)"
echo "members: $(unzip -Z1 "$OUT" | wc -l | tr -d ' ')"
echo "roots:   $(unzip -Z1 "$OUT" | awk -F/ '{print $1}' | sort -u | tr '\n' ' ')"
echo "symlinks in archive: $(unzip -Zl "$OUT" | grep -c '^l' || true)"
rm -rf "$STAGE"
