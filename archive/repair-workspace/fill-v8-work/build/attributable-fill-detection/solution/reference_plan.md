# Reference plan

## Before

The starting repository is not greenfield and the attribution architecture is not what is
missing. `src/engine/fill_classifier.rs` already holds an `AttributionLedger` with one
`SubmissionRecord` per submitted order keyed by `ClientRef`, already owns the feed cursor and
the set of absorbed `block_ref` values, already reads `FillEvent::at`, `FillBatch::blocks` and
`venue_seq`, and already divides a block over the sum of *outstanding* quantities with integer
truncation. `src/engine/cycle.rs` already folds `open_orders` and the fill feed into that
ledger every cycle, derives three state leg visibility, keys neutralizations by `ClientRef`,
sizes them from a residual gap, and persists the whole structure. `src/types/snapshot.rs`
already carries every durable field. All of it compiles untouched.

What is wrong is how seven of those observations are read. Each wrong reading carries a
plausible justification in a comment, and none of the comments says it is wrong.

In `src/engine/fill_classifier.rs`:

1. `in_window` closes the window at both ends — `at <= record.window_start +
   ATTRIBUTION_WINDOW`. The window is half-open, so an observation exactly at the upper bound
   must not count. One character.
2. `apply_event` folds a cumulative quantity in as an increment,
   `record.direct_filled += event.cumulative_filled_qty`. The feed is at least once and
   redelivers, so the record has to be a high-water mark.
3. The eligible set is sorted by `order_id` alone, on the stated grounds that the venue's
   `covers` order must not decide the outcome. That much is true; the identifier is the third
   key, not the first. Identifiers are opaque venue labels whose lexicographic order
   contradicts arrival order, so this produces a plausible wrong answer.
4. The truncation remainder is handed to the largest outstanding order, on the stated grounds
   that it has the most room. The rule places it one base unit at a time to the earliest
   orders in the documented ordering.

In `src/engine/cycle.rs`:

5. `close_gap` treats `LegVisibility::Unconfirmed` like `Withdrawn` and falls through to
   neutralization, because neither is resting at the venue. Absence is not proof of
   withdrawal: the order may have filled.
6. The gap is `leg.size - self.acquired_for(leg)`, which ignores quantity already working at
   the venue but not yet attributed, so consecutive cycles send the same exposure twice.
7. A lost submission acknowledgement drops the neutralization record entirely and leaves a
   later cycle to work out what is missing. The venue accepted an order it never named; the
   record has to be tracked with no `order_id` so the retry reuses the same `ClientRef`.

Three families the starting repository gets right, and which a rewrite can easily break:
the `absorbed_blocks` dedup guard on replayed blocks, the outstanding-quantity basis of the
division, and the durability of window starts, absorbed block references and the client
reference counter.

## After

Two files change: `src/engine/fill_classifier.rs` and `src/engine/cycle.rs`, together 81
additions and 107 deletions against the starting repository. Nothing outside fill processing
moves. `Engine::new` keeps its seven arguments, `run_cycle`, `positions` and `pending_orders`
keep their signatures, and no port changes.

`solution/fixed/` mirrors the whole set of crate paths `solution/solve.sh` installs, so the
three that are byte-identical to the starting repository are shipped as well and are documented
below as the architecture the candidate is given rather than work the candidate has to do:

```
solution/fixed/src/types/snapshot.rs           (identical to the starter)
solution/fixed/src/engine/fill_classifier.rs   (changed)
solution/fixed/src/engine/neutralization.rs    (identical to the starter)
solution/fixed/src/engine/cycle.rs             (changed)
solution/fixed/src/engine/mod.rs               (identical to the starter)
```

`solution/solve.sh` copies each one to the matching path under /app, runs `cargo fmt` when
rustfmt is present, and builds offline. Each section below names the destination path.

### 1. `src/engine/fill_classifier.rs` — attribution ledger

The ledger already exists: one `SubmissionRecord` per order the engine itself submitted, indexed
by `ClientRef`, fed from three sources. Four readings inside it change.

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

### 2. `src/types/snapshot.rs` — durable state (starter-provided, unchanged)

This file is byte-identical to the starting repository. It is described here because the
reasons behind its fields are what a rewrite has to preserve.

`SubmissionRecord` carries `client_ref`, `order_id`, `window_start`, `venue_seq`, `size`,
`direct_filled`, `block_filled` and `finalized`, with `attributed()` and `outstanding()` derived
from them. `Snapshot` carries `fill_cursor`, `submissions`, `absorbed_blocks`, `closing_orders`,
`replaced_orders`, `neutralizations` (a `NeutralizationRecord`), and `client_seq`.

`window_start` has to be durable. A restart that reopens a window lets a replayed observation
from outside the original window count, which is the misattribution the window exists to
prevent. `absorbed_blocks` has to be durable for the same reason in reverse: a forgotten
`block_ref` is a block counted twice.

`client_seq` is not incidental. A counter that resets to zero regenerates a client reference
the venue already knows, and the venue's deduplication then maps a new neutralization onto an
unrelated existing order.

### 3. `src/engine/neutralization.rs` — gap sized neutralization (starter-provided, unchanged)

This file is byte-identical to the starting repository. `plan_neutralization` already takes the
residual quantity it should cover and returns `None` when that
residual is not at least one base unit. Nothing rounds, so a residual is either a whole number
of base units worth covering or it is zero. What changes is the residual handed to it, in
`cycle.rs` below.

### 4. `src/engine/cycle.rs` — reconcile before acting

`run_cycle` already folds observations into the ledger before it does anything economic, and the
stage order is starter-provided:

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
`InventorySource` is not consulted for completion at all; it survives only as a cash
affordability check when entering a strategy. Both were already true in the starting repository.

**Visibility.** Hedging is gated by a three state rule derived from the open order list. The
enum and the derivation are starter-provided; the third row is what changes.

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
it, marks it `Withdrawn`, and neutralizes the residual gap. The extra cycle is what gives a fill
in flight room to land. Starter-provided.

**Idempotent neutralizations.** Every neutralization is written to `neutralizations` with a fresh
`ClientRef` and `order_id: None` *before* submission, which the starting repository already does.
Two things change. On `SubmitError::AcknowledgementLost` the record is now kept as is instead of
being dropped, so the next cycle resubmits under the same `ClientRef`, the venue deduplicates,
returns the original order, and the record resolves. And the gap now subtracts both attributed
quantity and outstanding neutralization quantity, where an unresolved record contributes its full
size and a record the venue has closed contributes nothing, so an in flight neutralization can
never be placed twice.

### 5. `src/engine/mod.rs` (starter-provided, unchanged)

Byte-identical to the starting repository. It re-exports `AttributionLedger`; there is no
`LegStatus`, `classify_legs` or `FILL_TOLERANCE` to remove.

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
