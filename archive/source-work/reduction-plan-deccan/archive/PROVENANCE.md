# Provenance — reduction-plan (accepted proposal "Recover Canonical In-Network Reduction Plans from Scale-Up Fabric")

| artifact | classification | note |
|---|---|---|
| ~/Downloads/reduction-plan-v1.zip | OLD-CONTRACT, REFERENCE-ONLY | toy contract, in-process V1 tests |
| ~/Downloads/reduction-plan-v1-trustfix.zip | OLD-CONTRACT, REFERENCE-ONLY | Class C workspace, 11 cases |
| ~/Downloads/benchmark-suite-next-final/reduction-plan-v1-final.zip | OLD-CONTRACT, REFERENCE-ONLY | starter shipped the full replay engine; 12 cases; schema 1.3 |
| ~/Downloads/benchmark-suite-9-final-work/reduction-plan/ | CURRENT-WORK of the old contract | byte-identical to the final ZIP |

All three ZIPs preserved untouched in Downloads; copies by SHA prefix in this directory. Nothing in the new package is patched from them: the frozen proposal changes the model (link occupancy ceil(bytes/rate) with latency after, ties by (ready tick, smallest origin id), one payload-sized accumulator per reduction, sink as final non-selectable reducer, JSON in/out with ok/invalid/infeasible, published plan_key).

## Current final (2026-09-10)
- `~/Downloads/reduction-plan-deccan-stage1-final.zip` — sha256 4f63b1971d0838fb3de81742bd85b79e3fd4325a7b0f8927b1d0eed840b487ab, 76064 bytes, 111 members, root `reduction-plan/`. CURRENT FINAL, built from `pkg/` after the final regression chain (`lab/final/`), TB4 rules applied (agent 28800, closing sentence, canary on line 1 of the six core files plus hedge line, no chmod on the reward file). Not uploaded.
