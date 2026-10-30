# FP8 artifacts (all OLD-CONTRACT, preserved untouched in ~/Downloads)
| path | sha256 (12) | bytes | members | root | mtime | status | contract |
|---|---|---|---|---|---|---|---|
| ~/Downloads/fp8-reshard-v1.zip | 5213ff9b14bf | 16521 | 33 | fp8-reshard | 2026-08-09 00:58 | SUPERSEDED | OLD-CONTRACT (V1 in-process harness) |
| ~/Downloads/fp8-reshard-v1-trustfix.zip | 5dd00c8ce1cb | 31611 | 43 | fp8-reshard | 2026-08-09 08:51 | SUPERSEDED | OLD-CONTRACT (Class C split) |
| ~/Downloads/benchmark-suite-next-final/fp8-reshard-v1-final.zip | 00db9c73c7b9 | 28568 | 23 | fp8-reshard | 2026-08-12 14:19 | REFERENCE-ONLY | OLD-CONTRACT (latest toy build; audited NO-GO on 2026-09-08); byte-identical to ~/Downloads/benchmark-suite-9-final-work/fp8-reshard/fp8-reshard/ |

## Current final (2026-09-09)
- `~/Downloads/fp8-reshard-deccan-stage1-final.zip` — sha256 20d2535b943a859a48d70c54b7d86806e16bbfd9873f115e87a1c2e04d0ed7a3, 129637 bytes, 121 members, root `fp8-reshard/`. CURRENT FINAL. Built from `pkg/` after the final regression chain (`lab/final/`) and certified by fresh extraction (`fresh/`). Not uploaded.
- The three old-contract ZIPs above are REFERENCE-ONLY history (toy contract); nothing in the final package was patched from them.

## 2026-09-10 — TB4 corrections before first upload
- `fp8-reshard-deccan-stage1-superseded-20d2535b.zip` (sha256 20d2535b…) superseded, never uploaded.
- `~/Downloads/fp8-reshard-deccan-stage1-final.zip` sha256 ef1bcaa1a30a0e0d6b3ded47871e17c3b55340c360c59b96c8a8178eee616a58, 129738 bytes, 121 members, root `fp8-reshard/`. Changes: `[agent] timeout_sec = 28800`; instruction ends with the mandated sentence; canary comment moved to line 1 of instruction.md; hedge `canary:` line in the six core files. No verifier, fixture, source or spec change. CURRENT FINAL.

## 2026-09-10 — reward-mode fix before first upload
- ef1bcaa1… superseded (never uploaded): its test.sh set chmod 600 on the re-created reward file, which blocks Harbor's non-root host reader (seen on the relocation v2 card as a G05 PermissionError). Archived as fp8-reshard-deccan-stage1-superseded-ef1bcaa1.zip.
- `~/Downloads/fp8-reshard-deccan-stage1-final.zip` sha256 5982e3b6cb6ad674ba91e5706e43755673c82002787e1ba313224f21170f7389. Only change: chmod 600 removed. CURRENT FINAL.
