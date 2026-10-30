# 12 — Behavioural invariant specification

This is the semantic core of the task. It is written in observable terms only: no data
structure, no helper, no algorithm.

## Defining "attributable"

> A quantity of tokens is **attributable** to a tracked order when the venue has reported
> that quantity as filled *against that order's identity*, and no other tracked order has
> already had the same quantity counted toward it.

Two clauses, both load-bearing:

- **against that order's identity** — rules out absolute balance, which names no order.
- **no other order has already counted it** — rules out counting one inflow twice, which
  is how a shared token id lets two markets consume each other's fills.

The consequence the whole task turns on: **an increase in a token's wallet balance is not
evidence that any particular tracked order filled.** It is evidence that *something*
happened to that token.

## Required per-order observable state

For every tracked order the engine must be able to answer, at any cycle:

| # | Question | Why it matters |
|---|---|---|
| 1 | Which market? | Shared token ids make market ≠ token |
| 2 | Which token? | Balance reads are keyed by token |
| 3 | Which side? | Pair completion needs both |
| 4 | Intended quantity? | Ceiling on what may ever be attributed |
| 5 | Acknowledged filled quantity? | Monotonic progress; the anti-double-count value |
| 6 | Attribution checkpoint? | Separates pre-existing inventory from this order's fills |
| 7 | Resting-order identity? | Cancel, reconcile, match events |
| 8 | Hedge identity, if any? | A hedge is a distinct order with its own fills |
| 9 | Accepted-but-unacknowledged submission? | Lost ack ≠ not submitted |
| 10 | Resolved exposure? | Drives whether a hedge is owed at all |
| 11 | Persisted across restart? | The wallet remembers; the process must too |

The task states these as *questions the engine must answer*, never as fields to add.

## Required distinctions

The engine must distinguish all eight, using only what the ports supply:

| Situation | Must be treated as |
|---|---|
| Historical balance present before tracking began | never a fill of any tracked order |
| A tracked resting order filling | a fill of that order, up to its intended quantity |
| A hedge order filling | a fill of the hedge, never of the original resting order |
| Another market's order filling on a shared token | a fill of that other market only |
| The same observation repeated | no new information; no new action |
| A delayed observation | not yet known; not "known to be zero" |
| A response lost after venue acceptance | possibly executed; never assume it was not |
| Restored state after restart | continuation, never a fresh start |

## Invariants I1–I14

**I1 — No phantom fills.** Token balance existing before an order is tracked never counts
toward that order.

**I2 — Hedge fills are not credited to the resting order.** A balance change caused by a
hedge must not increase the acknowledged fill of the leg it was hedging.

**I3 — Idempotent observation.** Repeating an identical observation, or re-delivering an
event already acted upon, produces no additional submission and no state change.

**I4 — Exactly one position per genuine pair.** When both tracked legs genuinely fill, a
single position is created, with sizes equal to the attributable filled quantities, and
zero hedge orders are submitted.

**I5 — At most one hedge per unresolved exposure.** A genuine one-sided fill produces at
most one economically distinct hedge for that exposure.

**I6 — Hedging remains available.** A later, distinct unresolved exposure is still
hedgeable. No permanent global disable.

**I7 — Exposure bounded under concurrency.** A fill arriving while a hedge is in flight must
not leave net holdings of the hedged token above the intended leg quantity.

**I8 — Accepted work is not repeated.** After a lost acknowledgement, the engine must not
cause a second economically distinct execution of the same hedge.

**I9 — Independent attribution across markets.** Two markets sharing a token id attribute
independently; neither consumes the other's fills.

**I10 — Restart continuity.** After restart, historical balances are not reinterpreted as
new fills, and unresolved live orders remain recoverable.

**I11 — No silent disappearance.** A live resting order leaves tracking only if it is
confirmed filled, confirmed cancelled, expired under the frozen `should_cancel()` rule, or
otherwise resolved. Dropping it while it is live at the venue is a violation.

**I12 — Hedge size equals unresolved exposure.** Never the full original leg when part is
already resolved.

**I13 — Bounded progress (liveness).** Once the information needed to resolve a market is
observable, the engine reaches the correct terminal state within a bounded number of
cycles. No unbounded waiting, no deadlock, no starvation of one market by another.

**I14 — Frozen behaviour preserved.** `should_cancel()` (300 s), `should_exit_now()`
(60..=120 s), `is_expired()`, `is_too_close_to_expiry()` (< 60 s), `Position::new`
arithmetic, and both sizing functions keep their exact semantics. The nine
`PendingOrderManager` and six `PositionManager` public methods keep their signatures and
meanings.

## Safety / liveness pairing

Every safety invariant is paired with a liveness invariant that forbids satisfying it by
doing nothing:

| Safety | Paired liveness |
|---|---|
| I1, I2, I9 no false fills | I4 genuine pairs still form |
| I3, I8 no duplicate hedges | I6 later exposures still hedged |
| I5 at most one hedge | I13 the hedge actually happens, promptly |
| I7 exposure bounded | I12 the shortfall is actually covered |
| I11 nothing dropped | I13 orders still reach a terminal state |

An implementation cannot pass by being maximally conservative, and cannot pass by being
maximally eager.

## Explicitly unchanged

Market discovery, pricing, sizing, drawdown, redemption, alerting, and the cancellation
lead-time rule are outside the task. Sizing and the time predicates are asserted frozen;
the rest are simply absent from the package.
