#!/usr/bin/env bash
set -euo pipefail
W=~/benchmark-suite-work/reduction-plan-deccan; OUT="$1"; STAGE="$(mktemp -d /tmp/pkgstage8-XXXXXX)"
cp -R "$W/pkg" "$STAGE/reduction-plan"
find "$STAGE" \( -name '.DS_Store' -o -name '*.orig' -o -name '*.rej' -o -name '__MACOSX' -o -name '*.pyc' \) -print -delete
rm -rf "$STAGE/reduction-plan/environment/build" "$STAGE/reduction-plan/tests/verifier/build"
if find "$STAGE" -type l -print -quit | grep -q .; then echo "SYMLINK PRESENT — refusing"; exit 1; fi
rm -f "$OUT"; ( cd "$STAGE" && zip -q -r -X "$OUT" reduction-plan )
echo "path:    $OUT"; echo "bytes:   $(stat -f%z "$OUT")"; echo "sha256:  $(shasum -a 256 "$OUT" | cut -d' ' -f1)"
echo "members: $(unzip -Z1 "$OUT" | wc -l | tr -d ' ')"; echo "roots:   $(unzip -Z1 "$OUT" | awk -F/ '{print $1}' | sort -u | tr '\n' ' ')"
echo "symlinks in archive: $(unzip -Zl "$OUT" | grep -c '^l' || true)"; rm -rf "$STAGE"
