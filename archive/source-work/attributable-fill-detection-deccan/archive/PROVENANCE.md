
## 2026-09-10 — TB4 correction (agent timeout 28800, mandated closing sentence, hedge canary line in the six core files)
- superseded: attributable-fill-detection-deccan-stage1-v4-50c27fb9-stage1-notready.zip
- CURRENT FINAL: `/Users/macbok/Downloads/attributable-fill-detection-deccan-stage1-final.zip` sha256 c59e07188911cb49977b7869d3ceaf60d14c40b6293131df91f8a02804f1bf02, 154863 bytes, 118 members, root `attributable-fill-detection/`. No verifier, fixture, source or spec change. Not uploaded.

- 2026-09-10 canary hedge changed to the colon form in the six core files; intermediate build archived as *-never-uploaded.zip. CURRENT FINAL: `/Users/macbok/Downloads/attributable-fill-detection-deccan-stage1-final.zip` sha256 af9b67151d34f83fdc67aa4225daf4c51350560d134a8dbda67c202a2b33906c, 154858 bytes, 118 members.

## 2026-09-10 — finding after the Stage 2 long-horizon failure of v5 (af9b6715…)
- v5 Stage 2: difficulty met at 1/8, median 45.5→70 turns, long_horizon not met.
- Reconstructing the 8 rollout finals and re-grading locally reproduced 1/8. The 8 hidden records the seven failures missed (v13, v20, v21, v22, v23, v24, v24b, v28) differ from those finals ONLY in the order of `pending|` lines: the verifier's record writer emitted `positions()` and `pending_orders()` in the engine's own return order, and the frozen contract fixes no order for either accessor. With the writer canonicalised (positions by strategy; pending by strategy, venue, instrument, reference) and the certified records regenerated from the reference (8 records change, line order only), 6 of 8 finals are fully correct; only slots 06 and 08 fail (window-boundary and compound cases).
- Conclusion: the platform's 1/8 was a hidden-convention artifact, not task difficulty. The corrected verifier (canonical records; isolation fails closed; judge calls under timeouts) is in pkg/ but NOT packaged or uploaded pending a decision: a legitimate version is expected to score about 6/8 and fail the difficulty gate.
