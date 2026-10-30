# 15 — Starting broken-state design

What the agent receives. The package compiles, the suite runs, several cases pass, and the
defect is real rather than a typo.

## The shipped defect, stated once

Per-order fill completion is inferred from an **unattributed, shared, absolute** wallet
balance. The engine asks "does this token's balance look big enough?" when the only sound
question is "how much has the venue reported filled against *this order*?"

Everything else follows: historical inventory reads as a fill, another market's fill on a
shared token reads as this market's fill, a hedge's own fill reads as the resting leg's
fill, and a restart re-reads the whole wallet as brand-new activity.

## The self-amplifying loop

The compensating hedge increases the very balance the faulty detector reads. So the shipped
code does not merely mis-detect once — it mis-detects, hedges, and the hedge makes the next
mis-detection *more* likely. On a one-sided fill the run does not settle; it accelerates.
This is why V3 fails at NOP with a growing count of taker submissions rather than a single
wrong one.

## The pre-correction — mandatory

The source repository compares **both** legs against a threshold derived from the **YES**
order's size:

```rust
let min_required = yes_order.size - 0.001;
let yes_filled = yes_balance >= min_required;
let no_filled = no_balance >= min_required;
```

With the asymmetric sizing path in `order_execution/mod.rs` (`(y, n)` computed from each
leg's own limit price, e.g. 50 and 6), this is a second, *independent*, and far shallower
bug: the NO leg is measured against the YES leg's size.

**The sanitized copy ships the per-leg form already corrected:**

```rust
let yes_filled = yes_balance + FILL_TOLERANCE >= yes_order.size;
let no_filled = no_balance + FILL_TOLERANCE >= no_order.size;
```

Each leg is now compared against its own size. The arithmetic is right. The *attribution* is
still absent — `yes_balance` and `no_balance` are absolute wallet reads that name no order.

This correction is applied **only** to the sanitized task copy. The original repository at
`/Users/macbok/Documents/poly-arb-engine` is not modified.

### Why the pre-correction is required

Without it, the first thing a competent agent notices is the leg mismatch. Fixing it makes
V2 pass and feels like a solution. The proposal review flagged exactly this: the task would
be graded on a one-line change. Pre-correcting removes the shallow answer and leaves the
architectural one.

V1 and V2 pass at NOP as the visible consequence — the agent sees that symmetric and
asymmetric pairs both work, which steers attention toward *when a fill is not a fill* rather
than toward *how big is big enough*.

## What else ships broken

Carried over from the source, deliberately:

**Two independent balance reads.** The YES and NO balances are fetched separately, so a fill
landing between them yields a torn pair. `balances_of` exists on the port (doc 06) but the
shipped code does not use it.

**`FillEventSource` wired but ignored.** The port is constructed, injected, and emits
well-formed per-order events with `seq` and `cumulative_filled_qty`. The shipped classifier
never polls it. The information needed for a correct solution is present from cycle 1 — the
task is to *use* it, not to invent it. This keeps the task solvable without guessing at an
API the agent cannot see.

**Full-leg hedging.** `attempt_orphan_hedge` submits a taker order for the entire
`unfilled.size`, not the shortfall (P8's origin).

**Fresh `ClientRef` on every submission attempt.** Any `Err` from `submit` is treated as
"not placed". `SubmitError::AcknowledgementLost` exists as a distinct variant and is matched
into the same arm as `Rejected` (P4's origin).

**Cancel failure ignored.** A failed `cancel` is logged and execution proceeds as though it
succeeded.

**Insufficient snapshot.** `Snapshot` records which orders exist but nothing about how much
of each has been acknowledged, and nothing separating engine-caused inventory from
pre-existing inventory (doc 09).

**No baseline anywhere.** No checkpoint is taken when tracking begins. There is nothing to
subtract from.

## What ships correct and must stay correct

Frozen per doc 05 and asserted by I14: `should_cancel()` (300 s), `should_exit_now()`
(60..=120 s), `is_expired()`, `is_too_close_to_expiry()` (< 60 s), `Position::new`
arithmetic (`entry_cost`, `total_usd_cost`), both sizing functions, and the nine
`PendingOrderManager` / six `PositionManager` public methods.

An agent that "simplifies" these fails the regression cases even with attribution solved.

## Scope decisions restated

**D1 — cancellation ordering excluded.** The source's `cancel_unfilled_leg` removes both
pending entries regardless of whether the cancel succeeded. In the sanitized copy the driver
calls cancel **before** cleanup, so this ordering hazard is not reachable and is not part of
the task. Including it would add a second unrelated defect.

**D2 — threshold pre-corrected.** Above.

**D3 — `market_question` dropped** everywhere; it carried production market text.

## Compile and run state on delivery

- `cargo build` succeeds with no warnings that would mask the defect
- `cargo test` runs the full suite offline
- V1 and V2 pass; V3–V13 fail
- No test is `#[ignore]`d, no case is commented out, nothing panics at startup

The starting state is a working program that is wrong, not a broken program.
