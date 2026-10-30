# 16 — Reference-solution design (design only, no implementation)

This document describes *what* a correct solution establishes and *why* each piece is
necessary. It contains no implementation, no `solve.sh`, and no solution source. The
implementation is blocked until the Official Contributor Guidelines define the solution
format.

The design is stated as obligations, not as a diff. Multiple implementations satisfy it, and
the verifier accepts any of them — it judges behaviour, never helper names or structure.

---

## The central move

Stop asking the wallet *whether* an order filled. Ask the venue *how much* it has reported
filled against that order, and treat the wallet only as a bound, never as evidence of
authorship.

Concretely: `FillEventSource` becomes the authority for attribution; `BalanceSource` is
demoted to a consistency check that can confirm an upper bound but can never establish that
a specific order filled.

---

## Obligation 1 — Baseline at the moment tracking begins

When an order enters tracking, record the observable state that separates "inventory that
already existed" from "inventory this order will produce". The checkpoint is taken once, at
tracking time, and is not re-sampled on subsequent observations.

*Covers.* Pre-existing wallet balance (V5), restart with historical inventory (V11).
*Rejects.* P5 (baseline at check time) and P1 (no baseline at all).

## Obligation 2 — Per-order, per-market attribution

Each tracked order carries its own acknowledged-filled quantity, advanced only by events
naming that order's identity. Two orders on the same token id advance independently; neither
consumes the other's reported fills.

*Covers.* Shared token id across markets (V10), independent markets (V12).
*Rejects.* P7 (first-market wins), P1 (token-level reasoning).

## Obligation 3 — Well-defined fill progress

Acknowledged quantity is monotonic non-decreasing and bounded above by the order's intended
quantity. Progress is derived from the event stream's cumulative value rather than by
summing deltas, so an event observed twice cannot advance it twice.

*Covers.* Partial fills (V6), redelivery (V4).
*Rejects.* Delta-accumulating solutions.

## Obligation 4 — Stale and duplicate observations are inert

An observation already incorporated produces no state change and no submission. This holds
for a repeated balance read, a redelivered fill event, and a frozen read window.

*Covers.* V4, V7.
*Rejects.* P6 — and note the rejection is by deadline, not by inspecting the solution for a
wait construct.

## Obligation 5 — Concurrent fill and hedge

A fill arriving while a hedge is in flight must not push net holdings of the hedged token
above the intended leg quantity, and must not trigger a second compensating order. This
requires that "a hedge exists for this exposure" is recorded at *submission* time, not at
confirmation time, while remaining reconcilable if the hedge is later found not to exist.

*Covers.* V8.
*Rejects.* Solutions that treat submission as resolution, and solutions that only record
confirmed hedges.

## Obligation 6 — Accepted-but-unacknowledged submissions

A lost acknowledgement means *possibly executed*. The solution must record the submission
identity before or atomically with the attempt, so that a subsequent attempt reuses the same
identity (venue-side dedupe) or reconciles against open orders rather than creating a second
economically distinct order.

*Covers.* V9, V9×V11.
*Rejects.* P4.

## Obligation 7 — Restart restoration

Whatever the solution treats as acknowledged must survive restart, coherently with the fill
cursor. After reload: historical balances are not new fills, live resting orders remain
recoverable, and in-flight hedges are not duplicated. A snapshot that failed to write must
not cause a double hedge either — the recovery path reconciles against the venue rather than
assuming the snapshot is authoritative.

*Covers.* V11, `FailStateSave` seeds in V13.
*Rejects.* In-memory-only solutions; incoherent cursor persistence in both directions.

## Obligation 8 — Shared token ids

Attribution is keyed by order identity, so a shared token id is not a special case. The
solution should require no branch for it. If a solution needs a shared-token special case,
it has probably not moved off token-level reasoning.

*Covers.* V10.

## Obligation 9 — Correctly scoped hedge idempotency

Idempotency is scoped to *an unresolved exposure*, not to the engine, not to a market for
all time. One exposure yields at most one economically distinct hedge; a later distinct
exposure yields its own.

*Covers.* V3, V4 (at most one) and V12 (a later one still happens).
*Rejects.* P2.

## Obligation 10 — Later legitimate hedging still occurs

The liveness half of Obligation 9, stated separately because it is what a flag-based
solution violates. Once an exposure is observable and unresolved, a hedge is submitted
within the cycle bound.

*Covers.* V12, V7, V13.
*Rejects.* P2, P9.

## Obligation 11 — Live resting orders are preserved

A tracked order leaves tracking only when confirmed filled, confirmed cancelled, expired
under the frozen rule, or otherwise resolved. Uncertainty is not grounds for dropping it. A
failed cancel means the order may still be live.

*Covers.* V5, V3, V11.
*Rejects.* P3.

## Obligation 12 — Final exposure bounds

At the end of every scenario, holdings of each token attributable to the engine's own
activity are within the intended quantities, and every genuine pair has produced exactly one
position with sizes equal to attributable filled quantities.

*Covers.* V1, V2, V6, V8, and all of V13.

---

## Explicitly not the solution

The following are ruled out by the scenario matrix, each by an observable outcome:

| Non-solution | Fails on |
|---|---|
| Disable hedging entirely | V3, V12 — no hedge is ever submitted, exposures never resolve |
| Clear state whenever uncertain | V5, V1 — live order dropped; genuine pair never forms |
| Sleep or retry until balances settle | V7 — frozen reads never change, deadline missed |
| One global lock over the engine | V12, V7 — independent markets miss the cycle bound |
| Permanent per-market "already hedged" flag | V12 — second exposure never hedged |
| Treat every balance increase as this order's fill | V5, V10, V11 — the shipped defect, restated |
| Inspect source patterns or engine internals | not reachable — the solution has no access to the harness |
| Hardcode fixture ids or schedule constants | V13 — ids and quantities vary per seed |
| Branch on being under test | V13 — no signal exists; the harness exposes none |

---

## Effort shape

The solution touches the attribution path, the hedge-tracking path, and the snapshot. It
does not require new dependencies, new ports, or changes to the frozen public API. A
competent Rust engineer who correctly identifies the defect has a substantial but bounded
implementation ahead: the difficulty is in the diagnosis and the invariant reasoning, not in
volume of code.

## Verification posture

The reference solution exists to prove the suite is passable and to establish the
oracle-passes half of the NOP/oracle gate. It is not a template the agent is graded against.
The verifier asserts on manager state and gateway history only — no helper name, no module
layout, and no internal type appears in any assertion.
