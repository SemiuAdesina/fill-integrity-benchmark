# Reference plan

## Before

`src/engine/fill_classifier.rs` decides whether a tracked leg has filled by asking the
custody account how many of the leg's instrument it currently holds:

```rust
let inventory = inventory.instrument_inventory(&leg.instrument_id)?;
if qty_at_least(inventory, leg.size) {
    LegStatus::Filled
} else {
    LegStatus::Open
}
```

The custody account inventory is shared, absolute, and lagging. It aggregates instruments from earlier
rounds, earlier process runs, other tracked strategies that happen to use the same instrument
identifier, neutralization orders, and positions the engine no longer tracks. Using it as proof that
one specific order filled is wrong in six independent ways, and the failures compound:

- An opening inventory completes a leg the moment it is placed.
- Two strategies sharing an instrument identifier complete each other's legs.
- A neutralization buys the same instrument the detector reads, so the corrective action raises the signal
  that triggered it. Once a neutralization lands, the inventory looks like a fill, a position is opened
  on instruments nobody bought for it, or a second neutralization is issued. This is a feedback loop.
- A frozen inventory source stalls progress if the inventory is the only evidence.
- A lost submission acknowledgement leaves an order the process cannot name, and a retry
  keyed on anything but the venue's idempotency key doubles the exposure.
- A restart clears process memory but not the custody account, so every historical inventory is new
  again.

`src/engine/neutralization.rs` compounds it by planning the neutralization for the full leg size rather than
the unresolved remainder, and `src/engine/cycle.rs` cancels and neutralizations in the same cycle, so
a fill already in flight has no chance to land before another economic order is sent.

An earlier, easier defect in this area — the per leg threshold compared the raw inventory
strictly, so an exactly filled leg never registered — is already corrected in the starting
repository. `classify_legs` already reads `inventory + FILL_TOLERANCE >= size`, with
`FILL_TOLERANCE` at zero because quantities are whole base units. The remaining problem is
therefore not reachable by adjusting a threshold or an epsilon.

The starting repository also has no notion of the attribution window or of block fills. It
never looks at `FillEvent::at`, never reads `FillBatch::blocks`, and never uses `venue_seq`,
so every rule in `/app/spec/venue-semantics.md` about when an observation counts and how a
shared match divides is unimplemented rather than merely wrong.

## After

Five files change. Nothing outside fill processing moves. `Engine::new` keeps its seven
arguments, `run_cycle`, `positions` and `pending_orders` keep their signatures, and no port
changes.

The corrected files live under `solution/fixed/`, mirroring their crate paths:

```
solution/fixed/src/types/snapshot.rs
solution/fixed/src/engine/fill_classifier.rs
solution/fixed/src/engine/neutralization.rs
solution/fixed/src/engine/cycle.rs
solution/fixed/src/engine/mod.rs
```

`solution/solve.sh` copies each one to the matching path under /app, runs `cargo fmt` when
rustfmt is present, and builds offline. Each section below names the destination path.

### 1. `src/engine/fill_classifier.rs` — attribution ledger

Replaces the inventory predicate with `AttributionLedger`: one `SubmissionRecord` per order the
engine itself submitted, indexed by `ClientRef`, fed from three sources.

- `FillEventSource::poll` events, which carry a *cumulative* quantity, so the record keeps a
  high-water mark rather than a sum. That is what makes the ledger idempotent under at least
  once delivery, replays, and repeated cursors.
- `FillBatch::blocks`, which are incremental, so they are added — and deduplicated by
  `block_ref`, which is the venue's idempotency key for a match.
- `OpenOrder::filled_qty`, point-in-time truth for an order that is still live.

Every timestamped observation is tested against the record's half-open window before it counts:
`at >= window_start && at < window_start + ATTRIBUTION_WINDOW`. The open order list carries no
timestamp and is exempt, which is stated in the spec and is not an oversight.

`absorb_block` implements the division rule. It builds the eligible set from `covers`, keeps
only orders it tracks that are in window with outstanding quantity above zero, sorts them by
`(window_start, venue_seq, order_id)`, computes `min(block_qty, basis)`, gives each order
`proportional_share(divisible, outstanding, basis)`, then hands the truncation remainder out one
base unit at a time in that same order. Every covered order is then finalized: a block closes
what it names, whatever it gave it.

The sort key is the whole point of the tie-break. `covers` order, submission order and container
order are all available and all wrong. An unknown `venue_seq` sorts last, so an order whose
acknowledgement was lost never displaces a confirmed one.

### 2. `src/types/snapshot.rs` — durable state

`SubmissionRecord` carries `client_ref`, `order_id`, `window_start`, `venue_seq`, `size`,
`direct_filled`, `block_filled` and `finalized`, with `attributed()` and `outstanding()` derived
from them. `Snapshot` gains `fill_cursor`, `submissions`, `absorbed_blocks`, `closing_orders`,
`replaced_orders`, `neutralizations` (a new `NeutralizationRecord`), and `client_seq`.

`window_start` has to be durable. A restart that reopens a window lets a replayed observation
from outside the original window count, which is the misattribution the window exists to
prevent. `absorbed_blocks` has to be durable for the same reason in reverse: a forgotten
`block_ref` is a block counted twice.

`client_seq` is not incidental. A counter that resets to zero regenerates a client reference
the venue already knows, and the venue's deduplication then maps a new neutralization onto an
unrelated existing order.

### 3. `src/engine/neutralization.rs` — gap sized neutralization

`plan_neutralization` takes the residual quantity it should cover and returns `None` when that
residual is not at least one base unit. Nothing rounds, so a residual is either a whole number
of base units worth covering or it is zero.

### 4. `src/engine/cycle.rs` — reconcile before acting

`run_cycle` now folds observations into the ledger before it does anything economic:

```
sync_observations   open_orders + fill feed into the ledger
reconcile_neutralizations    resolve or resubmit unacknowledged neutralizations
enter_new_strategies
reconcile_open_strategies
retire_stale_orders
persist
```

**Completion.** A leg is complete when the quantity attributed to its own `client_ref`, plus
the quantity attributed to neutralization orders for the same strategy and venue, reaches the
leg size. The comparison is exact integer `>=`: a leg short by one base unit is short.
`InventorySource` is no longer consulted for completion at all; it survives only as a cash
affordability check when entering a strategy.

**Visibility.** Hedging is gated by a three state rule derived from the open order list:

| State | Meaning | Action |
|---|---|---|
| `Resting` | still listed at the venue | may be closed |
| `Withdrawn` | this engine cancelled it | may be neutralized |
| `Unconfirmed` | absent, and we did not cancel it | never neutralized |

An order that vanishes from the list may have vanished because it filled. Hedging an
`Unconfirmed` leg is exactly the over neutralization the task is about, so the engine waits. If
`open_orders` itself fails, every leg becomes `Unconfirmed` and the cycle does nothing
economic.

**Two step closure.** The first cycle records the leg in `closing`. The next cycle cancels
it, marks it `Withdrawn`, and neutralizations the residual gap. The extra cycle is what gives a fill
in flight room to land.

**Idempotent neutralizations.** Every neutralization is written to `neutralizations` with a fresh `ClientRef` and
`order_id: None` *before* submission. On `SubmitError::AcknowledgementLost` the record is
kept as is; the next cycle resubmits under the same `ClientRef`, the venue deduplicates,
returns the original order, and the record resolves. The gap subtracts both attributed
quantity and outstanding neutralization quantity, and an unresolved record contributes its full size,
so an in flight neutralization can never be placed twice.

### 5. `src/engine/mod.rs`

Re-exports `AttributionLedger` in place of the removed `LegStatus`, `classify_legs` and
`FILL_TOLERANCE`.

## Why the obvious partial fixes do not pass

| Attempt | Fails on |
|---|---|
| Subtract a baseline inventory captured at order placement | the neutralization raises the inventory; v9, v13 |
| Per instrument baselines | two strategies share an instrument identifier; v10 |
| Event feed only, ignoring `open_orders` | withheld feed stalls or over neutralizations; v7 |
| Ledger without the visibility rule | neutralizes a leg that already filled; v13 |
| Neutralization deduplication keyed on anything but `ClientRef` | v12 |
| Disable neutralization entirely | v4, v6 |
| Clear pending state when uncertain | v3, v7, v11 |
| One permanent per strategy neutralization flag | v11 |
| Ignoring `FillEvent::at`, or treating the window as closed at both ends | v18, v18b, v19 |
| A closed window `[start, end]`, or an inclusive end | v19 |
| Floating point shares, or rounding a share to nearest | v20, v25, v25b |
| Largest-remainder allocation instead of earliest-first | v20 |
| Sorting the eligible set by `order_id` alone | v21, v22 |
| Sorting by `covers` order, submission order or map order | v20, v21, v22 |
| Counting a block by anything other than `block_ref` | v23 |
| Leaving a covered order live or still working after a block | v24, v25, v25b |
| Any single one of the above, on the compound case | v25, v25b |

The last row is the point of the compound case. It stacks the window boundary, the truncation,
three equally eligible orders, all three tie-break levels and a second block whose division
depends on the first block's exact residuals. Each rule decides the same observable, so getting
any one of them wrong changes the answer.
