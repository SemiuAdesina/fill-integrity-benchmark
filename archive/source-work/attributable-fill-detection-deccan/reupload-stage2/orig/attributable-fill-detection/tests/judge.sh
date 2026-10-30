#!/usr/bin/env bash
# Verifier-owned final judge. This process never builds, imports, links or calls
# candidate code. It compares bounded raw observation artifacts with private
# expected observations and writes a deterministic verdict.
set -uo pipefail

mode="${1:-}"
cases="${2:-}"
expected="${3:-}"
observed="${4:-}"
verdict="${5:-}"

[ -f "$cases" ] && [ -d "$expected" ] && [ -d "$observed" ] && [ -n "$verdict" ] || exit 2

total=0
failed=0
target=""
while read -r kind name extra; do
    case "$kind" in ''|'#'*) continue ;; esac
    [ -z "${extra:-}" ] || exit 2
    total=$((total + 1))
    target="$name"
done < "$cases"

: > "$verdict"
while read -r kind name extra; do
    case "$kind" in ''|'#'*) continue ;; esac
    exp="$expected/$name"
    got="$observed/$name"
    ok=1
    [ -f "$exp" ] && [ ! -L "$exp" ] || ok=0
    [ -f "$got" ] && [ ! -L "$got" ] || ok=0
    [ "$ok" -eq 0 ] || [ "$(wc -c < "$got")" -le 1048576 ] || ok=0
    if [ "$mode" = '--selftest' ] && [ "$name" = "$target" ]; then
        # Mandatory verifier-owned expected-failure control. The judge must
        # reject exactly one altered raw observation without candidate input.
        ok=0
    elif [ "$name" = 'v13b_seeded_orderings_are_repeatable' ] && [ "$ok" -eq 1 ] \
        && awk '
            BEGIN { block=""; previous=""; count=0; good=1 }
            { block=block $0 "\n" }
            $0=="END" {
                count++
                if (count % 2 == 1) previous=block
                else if (block != previous) good=0
                block=""
            }
            END { exit !(good && count==16 && block=="") }
        ' "$got"; then
        # This case asserts only repeatability between each pair of candidate
        # runs; it deliberately does not compare the candidate with reference.
        ok=1
    elif [ "$ok" -eq 1 ] && cmp -s "$exp" "$got"; then
        ok=1
    else
        ok=0
    fi
    if [ "$ok" -eq 1 ]; then
        printf 'PASS %s\n' "$name" >> "$verdict"
    else
        printf 'FAIL %s\n' "$name" >> "$verdict"
        failed=$((failed + 1))
    fi
done < "$cases"
printf 'TOTAL %s FAILED %s\n' "$total" "$failed" >> "$verdict"

if [ "$mode" = '--selftest' ]; then
    [ "$total" -eq 48 ] && [ "$failed" -eq 1 ]
else
    [ "$mode" = '--grade' ] && [ "$total" -eq 48 ] && [ "$failed" -eq 0 ]
fi
