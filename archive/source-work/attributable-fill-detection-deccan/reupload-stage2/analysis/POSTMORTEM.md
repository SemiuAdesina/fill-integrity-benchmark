# Trajectory postmortem: attributable-fill-detection, eval s2eval_3f52a9dd69843a919dd67b4b56af8602, arm hy4_difficulty

Source: exact replay of every Write/Edit/Bash tool call of each attempt inside the task image
(`replay/slotXX/`), verified against every later `Read` of a `/app/src` file in the same trajectory
(0 mismatches across 121 checked reads). Baseline replay against the original v10 verifier:
01=1 02=0 03=1 04=0 05=1 06=1 07=1 08=1 (6/8), identical to the platform result.

| slot | reward | turns | files changed | first architectural decision | first compiling build | repair cycles after a failing run | tests added |
|---|---|---|---|---|---|---|---|
| 01 | 1 | 25 | fill_classifier.rs, cycle.rs | keep the starter's ledger; rewrite both files in one pass (steps 11–12) after reading all spec + diagnostics | step 13 (first build) | 0 | tests/scratch.rs (11 own interaction tests) |
| 02 | 0 | 107 | fill_classifier.rs, cycle.rs | same shape; four-state visibility, window-aware `resolve`, audit subagents | step 33 | 3 | diagnostics.rs extended, reproduce.rs extended, /tmp probe crate |
| 03 | 1 | 63 | fill_classifier.rs, cycle.rs | same shape; client_ref-first `locate`, finalized-as-withdrawn | step 29 | 1 | diagnostics.rs rewritten, harness, tests/interactions.rs (9 tests) |
| 04 | 0 | 57 | fill_classifier.rs, cycle.rs, snapshot.rs (`submitted_at` on NeutralizationRecord) | same shape; window-aware `index_of_order_at`; literal three-state visibility | step 27 | 0 | diagnostics.rs edited |
| 05 | 1 | 47 | fill_classifier.rs, cycle.rs | same shape; `resolve_by_order_id` window-aware; finalized-as-closed | step 28 | 1 | diagnostics.rs edited |
| 06 | 1 | 46 | fill_classifier.rs, cycle.rs | same shape; a fourth "closed by venue" visibility state | step 21 | 1 | diagnostics.rs edited, harness |
| 07 | 1 | 40 | fill_classifier.rs, cycle.rs, snapshot.rs (`entry_attempts`) | same shape plus entry-leg recovery and entry latching only once both legs are live | step 17 | 1 | diagnostics.rs edited, tests/interactions.rs (8 tests), mutation-tested two behaviours |
| 08 | 1 | 81 | fill_classifier.rs, cycle.rs, snapshot.rs (`leg_attempts`) | same shape plus `retry_unconfirmed_legs`, four-state visibility | step 33 | 0 | diagnostics.rs rewritten, /tmp probe crate |

Per-slot models (from the reconstructed final code and the probes in `lab/probe/`):

| slot | fill evidence | direct fill | identity / recycling | block division | remainder | finalization | neutralization accounting | lost ack | restart | cross-mandate |
|---|---|---|---|---|---|---|---|---|---|---|
| 01 | per-order ledger, custody never consulted | high-water mark, both sources | client_ref first; id bound only when unknown; blocks by first-match id | outstanding basis, (ws, seq, id) | one unit each, first r | every covered tracked order; finalized orders stay eligible later (judgement call) | residual = size − attributed − working (lost ack = full, finalized = 0) | neutralization retried under same ref; entry leg dropped | all starter fields; clears closing/replaced on settle | per order |
| 02 | same | high-water | client_ref first with window test on binding; window-aware block claimant | same | same | excludes finalized | same, plus finalized-leg coverable | neutralization retried; entry leg dropped | starter fields | per order; fails v6/v15/v16/v13 (over-neutralizes an unconfirmed leg via its four-state rule) |
| 03 | same | high-water | client_ref first; will not adopt an id another record holds; blocks by first-match id | same | same | excludes finalized | same | neutralization retried; entry leg dropped | starter fields | per order |
| 04 | same | high-water | client_ref first; window-aware `index_of_order_at` for events and blocks | same | same | excludes finalized | residual with working; literal unconfirmed rule leaves block-closed leg uncovered | drops entry leg; neutralization retry keeps record but fails v9 (window moved by `submitted_at`) | starter + submitted_at | per order |
| 05 | same | high-water | client_ref first; id fallback prefers window-containing record | same | same | excludes finalized | same; finalized leg corrected | neutralization retried; entry leg dropped | starter fields | per order |
| 06 | same | high-water | client_ref first; blocks by first-match id | same | same | excludes finalized; dedups covers | same; fourth "closed" state corrected | neutralization retried; entry leg dropped | starter fields | per order |
| 07 | same | high-water | client_ref first; blocks by first-match id | same | same | excludes finalized | same; block-closed resting leg left unconfirmed (uncovered) | neutralization and entry leg retried under same ref (`entry_attempts`) | starter + entry_attempts | per order |
| 08 | same | high-water | client_ref first; blocks by first-match id | same | same | excludes finalized; finalized orders take no more direct fills | same; fourth "known-closed" state corrected | neutralization and entry leg retried under same ref (`leg_attempts`) | starter + leg_attempts | per order |

Why each passed or failed under the original v10 verifier:
- 01, 03, 05, 06, 07, 08: implemented all seven readings the diagnostics named and nothing the suite graded beyond them.
- 02: its four-state visibility treats a leg that left the list without a block as coverable after one cycle; fails v6, v15, v16, v13 (over-neutralization of quantity already owned).
- 04: `NeutralizationRecord.submitted_at` re-opens the window on retry so the deduplicated order's fill lands outside it; fails v9 and v13.

Why six independent implementations derived the full solution so quickly:
1. `instruction.md` "What has to be true" enumerated every implementation consequence (high-water mark, block_ref idempotency, half-open window at both ends, outstanding basis, deterministic remainder, finalize every covered order, residual formula with all three terms, unknown-state suppression, closure before correction, same-reference retry, rejection leaves exposure open, restart list, integer arithmetic).
2. `instruction.md` and `source-map.md` said the state and wiring were sound and the defects were confined to how observations are read in `fill_classifier.rs` and `cycle.rs`, and `source-map.md` listed the eleven decision points to check.
3. `tests/diagnostics.rs` failed 11/17 on the starter with messages that named the fix ("must not add quantity, only raise it", "each of the first r orders … receives one extra base unit", "the block ordering is (window start, venue_seq, order_id)", "retrying under a fresh client_ref asks for a second order", "quantity still working … must not be neutralized a second time", "absent … not cancelled is unconfirmed, not withdrawn"), and drove the ledger's internal API directly.
4. All eight runs therefore read spec → one-pass rewrite of the two named files → build → diagnostics green → own tests → done. No run changed a graded outcome after its first compiling build (see `SNAPSHOT_MATRIX.txt`).

Contract corners the six passers actually disagree on (all ungraded in v10): a block-closed resting leg (01/03/05/06/08 cover the gap; 07 leaves it unconfirmed), a lost acknowledgement on an entry leg (07/08 recover it; the rest drop it), a recycled identifier named by a block (02/04/05 resolve it at the block instant; the rest take the oldest holder), finalized orders in a later block's eligible set (01 keeps them), and identifier-keyed closure state after settlement (01 clears it; 03/05/06 leave it and fail the final v26).
