# 13 — Verifier scenario matrix V1–V13

Every case: starting state, schedule, expected observables, invariant protected, partial fix
rejected. All assertions read manager state and gateway history. **No case inspects source
code.**

Leg sizes come from the real sizer: `calculate_minimum_order_size(0.10, 5.0) = 50` and
`(0.87, 5.0) = 6`. Symmetric cases use 30/30.

---

## V1 — Symmetric pair, both legs fill

**Start.** Zero balances. Market `m1`: YES 30 @ 0.45, NO 30 @ 0.52, both resting.
**Schedule.** c1: fill YES 30, fill NO 30. Run 4 cycles.
**Expect.** Exactly 1 position; `size_yes == 30`, `size_no == 30`; `entry_cost == 0.97`;
`total_usd_cost == 29.10`; **zero** taker submissions; both tracked orders removed.
**Protects.** I4, I14.
**Rejects.** Any solution that cannot form a position at all — including
over-conservative ones and P3.
**NOP.** Passes. This is the case the shipped code gets right, and it is why NOP is not
trivially zero on every case.

---

## V2 — Asymmetric pair, both legs fill

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87.
**Schedule.** c1: fill YES 50, fill NO 6. Run 4 cycles.
**Expect.** Exactly 1 position; `size_yes == 50`, `size_no == 6`; **zero** taker
submissions.
**Protects.** I4.
**Rejects.** The shipped per-leg threshold defect — and, critically, this case is what P1
*fixes*, so V2 alone is not sufficient difficulty. It exists to confirm the pre-correction
is genuinely in place and that asymmetry alone no longer breaks anything.
**NOP.** In the shipped state the threshold is pre-corrected, so this **passes** at NOP.
Deliberate: it prevents the agent from thinking the threshold is the problem.

---

## V3 — Asymmetric pair, one leg fills

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87. Hedge ask for NO 0.85.
**Schedule.** c1: fill YES 50 only. Run 6 cycles.
**Expect.** Exactly **one** economically distinct taker submission, side NO,
`qty == 6`; no position until the hedge fills; if it fills, exactly 1 position; YES tracking
never dropped while live.
**Protects.** I5, I12, I13.
**Rejects.** P8 (hedges 50 instead of 6). Also any solution that hedges repeatedly.
**NOP.** **Fails** — the shipped code hedges, the hedge raises the NO balance, and the next
cycle re-reads and hedges again.

---

## V4 — Stale observation repeats across cycles

**Start.** As V3.
**Schedule.** c1: fill YES 50, then `FreezeBalanceReads { cycles: 5 }` and
`RedeliverFillEvent` for the YES fill on c2, c3, c4. Run 8 cycles.
**Expect.** Still exactly **one** economically distinct NO taker submission across all 8
cycles; acknowledged YES fill stays 50, never 100 or 150.
**Protects.** I3, I5.
**Rejects.** Any solution accumulating `filled_qty_delta` without regard to redelivery, and
any that re-derives "unfilled" from a repeated balance read.
**NOP.** **Fails** — one hedge per cycle.

---

## V5 — Pre-existing wallet balance before the new pair

**Start.** Balance: YES token of `m1` already holds **40** from a prior round. `m1`: YES 30
@ 0.45, NO 30 @ 0.52, both freshly tracked and both live.
**Schedule.** c1: fill NO 30 only. YES never fills. Run 6 cycles.
**Expect.** **Zero** positions — the 40 historical YES tokens are not a fill. Exactly one
NO-side… (no: the *unresolved* side is YES) → exactly one **YES** taker hedge of `qty == 30`,
or the YES resting order explicitly cancelled through the gateway. The YES resting order is
**either still tracked or explicitly cancelled** — never silently dropped.
**Protects.** I1, I11.
**Rejects.** P1 decisively — a per-leg-correct threshold still sees 40 ≥ 30 and declares
YES filled. Also P3 (clearing pending state loses the live YES order).
**NOP.** **Fails** — shipped code reports `BothFilled{30, 30}`, creates a phantom position,
and removes the live YES order from tracking.

---

## V6 — Genuine partial quantity fill

**Start.** Zero balances. `m1`: YES 30 @ 0.45, NO 30 @ 0.52.
**Schedule.** c1: fill YES 30, fill NO **18**. Run 6 cycles.
**Expect.** Exactly one taker submission, side NO, `qty == 12` — the shortfall, not 30.
No position while NO is short. After the hedge fills, one position with `size_no == 30`.
**Protects.** I12, I13.
**Rejects.** P8 (full-leg hedge → 30, over-buying by 18). Also
`min(yes.size, no.size)`-style thresholds, which would declare the pair complete at 18.
**NOP.** **Fails** — shipped code hedges the full `unfilled.size`.

---

## V7 — Balance reads lag by deterministic cycles

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87.
**Schedule.** c1: fill YES 50 at the venue, `FreezeBalanceReads { cycles: 3 }`,
`WithholdFillEvents { cycles: 2 }`. Reads/events become truthful at c4. Run 10 cycles.
**Expect.** No hedge before the information is observable; **exactly one** hedge after; the
market reaches its terminal state by c10.
**Protects.** I3, I13.
**Rejects.** P6 — a sleep/retry-until-stable loop never advances `FakeClock`, so it observes
the frozen value forever and misses the c10 deadline. Rejected by deadline, not by scanning
for `sleep`.
**NOP.** **Fails** — hedges during the frozen window, repeatedly.

---

## V8 — Real fill arrives during hedge submission

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87.
**Schedule.** c1: fill YES 50. c2: engine submits the NO hedge; the *same* cycle the
original NO resting order fills 6 at the venue. Run 8 cycles.
**Expect.** Final NO holdings **≤ 6**; if the hedge also executed, no *second* compensating
order is submitted; at most one position, `size_no == 6`.
**Protects.** I7, I2.
**Rejects.** Any solution that credits the hedge's fill to the resting leg and then hedges
again; any that treats "hedge submitted" as "leg resolved" without reconciling.
**NOP.** **Fails** — over-exposed and hedges again.

---

## V9 — Accepted hedge, acknowledgement lost

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87.
**Schedule.** c1: fill YES 50. c2: `LoseAcknowledgement { next_submissions: 1 }` — the
hedge is created at the venue, the response is dropped. c4: the lost hedge fills 6. Run 10
cycles.
**Expect.** Exactly **one** economically distinct taker order exists at the venue. A retry
under the *same* `client_ref` is permitted and counts once. A resubmission under a fresh
`client_ref` creates a second order → fail. Final NO holdings `== 6`. One position.
**Protects.** I8.
**Rejects.** P4 decisively.
**NOP.** **Fails** — shipped code treats the error as "not placed" and resubmits fresh.

---

## V10 — Two markets share one token id

**Start.** Zero balances. `m1`: YES `tA` **20** @ 0.45, NO `tB` 20 @ 0.52. `m2`: YES **`tA`**
20 @ 0.40, NO `tC` 20 @ 0.57. Both markets fully tracked.

`m1`'s YES size is deliberately equal to the quantity `m2` will fill, so that the shipped
code's balance read crosses `m1`'s threshold exactly. Without this the misattribution would
sit below the threshold and the case would pass at NOP for the wrong reason.

**Schedule.** c1: fill only `m2`'s YES order (20 of `tA`). Run 8 cycles.
**Expect.** `m1` sees **no** YES fill despite the `tA` balance rising to 20. `m2` YES
acknowledged 20, and `m2`'s unresolved NO exposure of 20 is hedged exactly once. `m1`
produces zero positions, zero hedges, and keeps both orders tracked.
**Protects.** I9, I1.
**Rejects.** P7 (first-checked-market attribution). Note also that the shipped pairing map
is keyed by market with non-deterministic iteration order, so a fix that attributes to
"whichever market is examined first" is not merely wrong but unstable across runs.
**NOP.** **Fails** — `m1` reads `tA == 20 ≥ 20`, declares its YES leg filled, and hedges a
market that never traded.

---

## V11 — Restart with historical balances and persisted pending state

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87.
**Schedule.** c1–c2: fill YES 50; engine acts and persists. c3: `Restart`. Venue keeps the
50 YES tokens and the live NO resting order. Run to c12.
**Expect.** After restart: **no** new position invented from the 50 historical YES tokens;
the YES leg's already-acknowledged 50 is not re-hedged; the live NO resting order remains
recoverable — tracked or explicitly cancelled; total economically distinct taker orders
across the whole run **≤ 1**.
**Protects.** I10, I1, I8, I11.
**Rejects.** In-memory-only solutions; any that persist a fill cursor incoherently with
acknowledged quantities.
**NOP.** **Fails** — post-restart the engine has no memory and re-derives everything from
balance.

---

## V12 — Later genuine orphan after an earlier hedge

**Start.** Zero balances. `m1`: YES 50 @ 0.10, NO 6 @ 0.87. `m2`: YES 25 @ 0.20, NO 7 @ 0.75.
**Schedule.** c1: fill `m1` YES 50 → engine hedges `m1` NO. c3: hedge fills; `m1` completes.
c5: fill `m2` YES 25 only — a **new, distinct** orphan. Run 12 cycles.
**Expect.** `m1`: one hedge, one position. `m2`: **one** hedge of `qty == 7` submitted after
c5. Total 2 economically distinct hedges — one per exposure.
**Protects.** I6, I5.
**Rejects.** P2 decisively — a one-shot global boolean hedges `m1` and then never hedges
`m2`, leaving `m2` permanently orphaned.
**NOP.** **Fails** — for a different reason: over-hedging on both markets.

---

## V13 — Seeded randomised schedules

**Start / schedule.** Generated per doc 11 from fixed seeds. Suggested 64 seeds; final count
set during validation to keep runtime well inside the verifier budget.
**Expect.** For every seed, all applicable invariants I1–I13 hold, and the run reaches the
generator's independently computed terminal state within the cycle bound.
**Protects.** All.
**Rejects.** Fixes tuned to V1–V12's specific shapes; P10 (hardcoded fixture ids or
schedule values) fails as soon as ids differ per seed.
**NOP.** **Fails** on the large majority of seeds.

---

## Coverage summary

| Invariant | Cases |
|---|---|
| I1 no phantom fills | V5, V10, V11, V13 |
| I2 hedge fills not credited to resting leg | V8, V13 |
| I3 idempotent observation | V4, V7, V13 |
| I4 exactly one position per pair | V1, V2, V3, V6, V13 |
| I5 at most one hedge per exposure | V3, V4, V12, V13 |
| I6 hedging remains available | V12 |
| I7 exposure bounded | V8, V13 |
| I8 accepted work not repeated | V9, V11, V13 |
| I9 independent attribution | V10, V13 |
| I10 restart continuity | V11, V13 |
| I11 no silent disappearance | V3, V5, V11 |
| I12 hedge size = shortfall | V3, V6 |
| I13 bounded progress | V3, V6, V7, V12, V13 |
| I14 frozen behaviour | dedicated regression cases + V1 |

Every invariant has at least one dedicated case and at least one randomised case.

## Cases that pass at NOP, on purpose

V1 and V2. If every case failed at NOP, the agent would suspect the whole classification
path rather than the attribution question specifically. V1 and V2 passing establishes
"symmetry and asymmetry are handled; something else is wrong" — which is the intended
starting insight. The overall NOP score is still well below any pass threshold.
