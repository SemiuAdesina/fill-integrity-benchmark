# 14 — Partial-fix rejection matrix

Ten approaches an agent may plausibly reach that are *partially* right. Each must fail on
**observable behaviour**, never on source inspection. No case greps for `Mutex`, `sleep`,
`global_lock`, `hedged`, or any other token.

Reading key: **Caught by** lists the cases that fail; **Mechanism** is what the harness
observes, in terms a scenario can assert.

---

## P1 — Correct each leg's threshold, keep using absolute balance

The agent compares the YES balance to the YES size and the NO balance to the NO size, fixing
the cross-leg mismatch. Arithmetic now correct; attribution still absent.

**Caught by.** V5 (decisive), V10, V11, V13.
**Mechanism.** V5 starts with 40 pre-existing YES tokens against a fresh 30-share YES order.
`40 ≥ 30` is true under *any* threshold that reads absolute balance, so the engine declares
a fill that never happened, creates a phantom position, and drops a live resting order.
V10 does the same across markets: `m1`'s YES threshold of 20 is crossed by `m2`'s fill of
the shared token `tA`. V11 reproduces it across a restart with no fill at all.
**Why this matters most.** P1 is the fix the shipped code's *pre-corrected* state has
already applied (doc 15). An agent that "discovers" P1 has discovered something already
true, and V5 tells it so immediately.

---

## P2 — One-time boolean permitting only one hedge ever

A `hedged: bool` on the engine, or a per-market flag never cleared. Stops the runaway loop
in V3 and V4, so it looks like a fix.

**Caught by.** V12 (decisive), V13.
**Mechanism.** V12 completes `m1` (hedge fills, position forms) and then produces a
genuinely new orphan on `m2` at cycle 5. The expected observable is **two** economically
distinct hedges, one per exposure. A one-shot flag submits one and leaves `m2` permanently
unhedged — the run ends with `m2` holding a one-sided position and no compensating order in
the gateway log.
**Invariant.** I6 (hedging remains available) paired against I5 (at most one per exposure).
Neither alone is satisfiable by a flag.

---

## P3 — Clear all pending orders whenever observations disagree

"If balance and tracking conflict, reset." Superficially safe: no double hedge, no phantom
position.

**Caught by.** V5, V1, V11, V13.
**Mechanism.** V5 asserts the live YES resting order is *either still tracked or explicitly
cancelled through the gateway* — never silently dropped. Clearing state drops it while it is
still live at the venue, and the gateway log shows no cancel. V1 catches the eager form of
this: clearing on any disagreement prevents the legitimate symmetric pair from ever forming
a position, so `positions.len() == 0` against an expected 1.
**Invariant.** I11 (no silent disappearance), paired with I4.

---

## P4 — Retry hedge submission blindly after a lost response

`Err(_)` from `submit` is treated as "not placed", so the engine submits again.

**Caught by.** V9 (decisive), V11, V13.
**Mechanism.** V9 loses the acknowledgement of a hedge that the venue *did* create. The
harness counts **economically distinct** orders: a retry under the same `ClientRef`
resolves to the original order and counts once; a fresh `ClientRef` creates a second order
and counts twice. Blind retry produces two, and the final NO holdings reach 12 against an
intended 6.
**Note.** The gateway makes the correct path available — `SubmitError::AcknowledgementLost`
is a distinct variant, and `open_orders` can be reconciled. Nothing about V9 requires
guessing.
**Invariant.** I8.

---

## P5 — Take the baseline when checking fills rather than when tracking begins

The agent introduces a checkpoint, but samples it inside the fill check instead of at the
moment the order starts being tracked. Every cycle re-baselines to the current balance, so
the delta is almost always zero.

**Caught by.** V3, V6, V12, V13.
**Mechanism.** Two failure directions, both observable. If the baseline is re-sampled every
cycle, a genuine fill that lands between two checks is absorbed into the new baseline and
never attributed — V3 ends with the YES leg unfilled in the engine's view, no hedge is
submitted, and the market never reaches a terminal state within the cycle bound (I13
liveness deadline). If instead the baseline is sampled once at first *check* rather than at
*tracking*, V5's 40 pre-existing tokens are captured correctly by luck but V6's partial fill
of 18 is measured against the wrong origin and the hedge quantity is not 12.
**Invariant.** I13 paired with I12.

---

## P6 — Sleep and reread until balances appear stable

A retry loop, a `tokio::time::sleep`, a spin, or "poll until two consecutive reads agree".

**Caught by.** V7 (decisive), V13.
**Mechanism.** `FakeClock` advances **only** when the driver advances it, and
`FreezeBalanceReads` pins the reported value for a fixed number of cycles regardless of how
many times it is read within a cycle. A stabilisation loop therefore observes the identical
value forever, consumes the cycle, and never converges. V7 requires the market to reach its
terminal state by cycle 10 after the reads become truthful at cycle 4; the loop misses it.
**Explicitly not** a source scan for `sleep`. The suite never reads the solution's text; the
schedule simply makes waiting useless and the deadline makes it fatal.
**Invariant.** I13.

---

## P7 — Attribute all token changes to whichever market is checked first

The engine iterates markets and lets the first one that could plausibly claim a balance
increase consume it.

**Caught by.** V10 (decisive), V13.
**Mechanism.** V10 gives `m1` and `m2` the same YES token `tA` and fills only `m2`. First-
match attribution credits whichever market the iteration reaches first. When that is `m1`,
the engine creates a phantom fill on a market that never traded and hedges it — the gateway
log shows a taker order for `m1`'s NO token, which the expectations forbid outright.
**Determinism note.** The shipped pairing is a `HashMap` keyed by market id, so iteration
order is not stable. P7 is therefore not merely wrong but *non-reproducibly* wrong; V13's
seeds surface it even when V10's ordering happens to be favourable.
**Invariant.** I9.

---

## P8 — Hedge the full original leg rather than only unresolved exposure

Carried over from the shipped `attempt_orphan_hedge`, which places a taker order for the
entire `unfilled.size`.

**Caught by.** V6 (decisive), V3, V13.
**Mechanism.** V6 fills YES 30 and NO 18. The unresolved exposure is 12. A full-leg hedge
submits 30, and the assertion on the gateway log — side NO, `qty == 12` — fails directly. If
the hedge executes, the resulting holdings are 48 against an intended 30, which also trips
the exposure bound.
**Invariant.** I12, with I7 as the backstop.

---

## P9 — Serialize the entire engine so required progress cannot happen

A single lock held across the whole cycle, an ordering constraint that makes one market wait
on another, or a queue that only drains on a condition that never fires.

**Caught by.** V12, V7, V13.
**Mechanism.** Every liveness assertion is expressed as "reaches terminal state within N
cycles", and `run_cycle` is invoked exactly once per cycle by the driver. Over-serialisation
shows up as a market that is still unresolved at the bound. V12 is the clearest: `m1` and
`m2` are independent, and `m2`'s orphan at cycle 5 must be hedged by cycle 12 regardless of
what `m1` is doing.
**Not caught by.** Anything resembling a lock-count or a source pattern. The harness is
single-threaded; the failure is purely "the work did not get done in time".
**Invariant.** I13.

---

## P10 — Hardcode visible fixture ids or schedule values

Branching on a market id, token id, order id, quantity, or cycle number seen in a test.

**Caught by.** V13 (decisive).
**Mechanism.** V13 generates schedules from fixed seeds via the bounded grammar in doc 11.
Ids are synthesised per seed and never appear in the hand-written cases; `OrderRef` names
orders by *role* rather than id precisely so that no literal id is ever visible to be
copied. Quantities are drawn from the real sizer across a range of price pairs, so `50`,
`6`, `30`, and `12` are not universal constants. Any branch keyed to a literal fails on the
majority of seeds.
**Secondary.** The hidden-test posture (pending the official guide) means the agent cannot
assume the visible cases are the graded ones.

---

## Coverage cross-check

| Partial fix | Decisive case | Supporting cases | Invariant |
|---|---|---|---|
| P1 absolute balance | V5 | V10, V11, V13 | I1 |
| P2 one-shot flag | V12 | V13 | I6 |
| P3 clear on conflict | V5 | V1, V11, V13 | I11, I4 |
| P4 blind retry | V9 | V11, V13 | I8 |
| P5 baseline at check time | V3 | V6, V12, V13 | I13, I12 |
| P6 sleep until stable | V7 | V13 | I13 |
| P7 first-market attribution | V10 | V13 | I9 |
| P8 full-leg hedge | V6 | V3, V13 | I12, I7 |
| P9 over-serialisation | V12 | V7, V13 | I13 |
| P10 hardcoded fixtures | V13 | — | all |

Every partial fix has a decisive case, and every decisive case is behavioural.

## Compound partial fixes

The realistic failure is a *combination* — for example P1 + P2, which fixes the arithmetic
and suppresses the loop, and passes V1, V2, V3, V4, and V6. It still fails V5 (phantom fill
from historical balance), V10 (cross-market), V11 (restart), and V12 (second exposure never
hedged). The matrix is designed so that no subset of P1–P10 clears the full suite: the only
approach that does is genuine per-order attribution.
