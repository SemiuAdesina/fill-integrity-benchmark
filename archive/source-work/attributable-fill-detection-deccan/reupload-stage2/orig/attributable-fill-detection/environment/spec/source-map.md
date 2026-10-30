# Source map

Where things are, so that finding them is not the exercise. What the engine must
*do* is in `venue-semantics.md`, `reconciliation-contract.md` and
`frozen-contract.md`; this file only says where each concern currently lives.

Nothing here is a solution sketch. It does not say which of the readings below is
right. The repair itself is the task.

## Entry point

`Engine::run_cycle` in `src/engine/cycle.rs` is the only thing the graded suite
calls. Every behaviour below is reached from it. Read it first: it is the whole
control flow, and the rest of the crate is types and helpers it calls.

One cycle, in the order it runs today:

```
run_cycle
├── ensure_loaded              restore Snapshot on the first cycle   cycle.rs
├── sync_observations          fold open_orders, then poll the feed  cycle.rs → fill_classifier.rs
├── reconcile_neutralizations  match up unacknowledged corrections   cycle.rs
├── enter_new_strategies       size and submit both legs             cycle.rs → sizing.rs
├── reconcile_open_strategies  decide what completed, and act        cycle.rs
│   └── close_gap              cancel, size the gap, cover it        cycle.rs → neutralization.rs
├── retire_stale_orders        cancel near expiry                    cycle.rs → managers.rs
└── persist                    write Snapshot back                   cycle.rs
```

## Which concern lives where

| Concern | File |
|---|---|
| Observations arrive | `ports/mod.rs` — `FillEventSource::poll` returning `FillBatch`, `OrderGateway::open_orders` |
| Observations are consumed | `engine/cycle.rs` — `sync_observations` |
| Per-order attribution state | `engine/fill_classifier.rs` — `AttributionLedger`, one `SubmissionRecord` per order |
| An observation is matched to an order | `engine/fill_classifier.rs` — `locate` |
| The attribution window is tested | `engine/fill_classifier.rs` — `in_window` |
| A per-order fill event is folded in | `engine/fill_classifier.rs` — `absorb_event` |
| A block is divided across orders | `engine/fill_classifier.rs` — `absorb_block` |
| A block is not counted twice | `engine/fill_classifier.rs` — `absorbed_blocks` |
| The live order list is folded in | `engine/fill_classifier.rs` — `absorb_open_orders` |
| A leg is judged complete | `engine/cycle.rs` — `acquired_for`, `leg_is_complete` |
| Whether a leg is still at the venue | `engine/cycle.rs` — `visibility`, `LegVisibility` |
| A position is created | `engine/cycle.rs` — `open_position` |
| A corrective order is sized and sent | `engine/cycle.rs` — `close_gap` |
| A corrective order is priced | `engine/neutralization.rs` — `plan_neutralization`, `sizing.rs` |
| Corrective orders already committed to | `engine/cycle.rs` — `reconcile_neutralizations`; `NeutralizationRecord` |
| Custody inventory is read | `ports/mod.rs` — `InventorySource` (affordability) |
| Orders being tracked | `engine/managers.rs` — `PendingOrderManager` |
| Positions held | `engine/managers.rs` — `PositionManager` |
| What survives a restart | `types/snapshot.rs` — `Snapshot`, via `StateStore` |
| Restart restore path | `engine/cycle.rs` — `ensure_loaded`, and `persist` |
| Feed position across cycles | `types/snapshot.rs` — `Cursor`, owned by the ledger |
| The attribution interval | `types/pending_order.rs` — `attribution_start`, `attribution_end`, `ATTRIBUTION_WINDOW` |
| Exact integer division | `types/ids.rs` — `proportional_share`, `Qty`, `QTY_UNIT` |

## Where a reading of the venue is chosen

The ledger and the cycle are plumbing: they carry observations in, hold state per
order, and write it back. The behaviour comes from a small number of places where
the code commits to *one* reading of what the venue said. Each of these is a
decision, and each has an answer in `venue-semantics.md` or in
`reconciliation-contract.md` — the first covers what the venue guarantees, the
second what the engine must do about it. Each is worth checking against those
documents rather than against how plausible the code looks.

1. `in_window` — which instants an order's window contains, at both ends.
2. `absorb_event` — what a cumulative quantity means when the feed replays it or
   delivers it out of order.
3. `absorb_open_orders` — what `filled_qty` on a live order means, and whether a
   window applies to an entry carrying no timestamp.
4. `absorb_block` — which covered orders are eligible, what the basis is, how the
   total divides, and where the base units left by truncation go.
5. `absorbed_blocks` — what makes two deliveries the same match.
6. `locate` — which key identifies one of the engine's own orders.
7. `acquired_for` — which submissions count toward one leg.
8. `close_gap` — how large the remaining exposure is, given what is already
   attributed and what is already working.
9. `visibility` — what an order's absence from `open_orders` does and does not
   prove.
10. `reconcile_neutralizations` — what to do about an order the engine sent and
    the venue never acknowledged.
11. `ensure_loaded` and `persist` — which state has to be durable for the next
    cycle to read the feed correctly.

## Files you are expected to change

Under `src/engine/` and `src/types/snapshot.rs`. `frozen-contract.md` is
authoritative on what is frozen; in short, the engine internals are yours and
`Snapshot` may gain fields.

You may reorganise those internals as you like: add modules under `src/engine/`,
split a file into a subdirectory, rename, or leave the layout exactly as it is.
`src/lib.rs` must still expose the frozen surface. Nothing is graded on file
layout, and there is no expectation that the work lands in one file.

Not to be changed: `src/ports/mod.rs` and the rest of `src/types/`. Both are
frozen, and the graded suite drives the engine through them.

`src/bin/reproduce.rs` and everything under `tests/` are diagnostics. Neither is
graded — `src/bin/` is discarded before grading and `tests/` is never read. Edit
either freely.

## Shared instruments

`StrategyCandidate` carries an `InstrumentId` per venue and two mandates may
carry the same one. `instrument_inventory` aggregates across mandates, across
runs, and across orders no longer tracked, so it cannot separate them. The
reproducer's second scenario demonstrates exactly this.

## Checking your work

```sh
cargo build --offline
cargo test --offline --test diagnostics
cargo run --offline --bin reproduce
```

`tests/diagnostics.rs` checks one documented rule at a time on synthetic values,
and each failure names the rule it broke rather than a case number. It covers the
window at both ends, cumulative quantities and redelivery, integer block
division, the remainder tie-break, the ordering, block idempotency, the open-order
list, lost acknowledgements, unconfirmed orders, and what has to survive a
restart. `tests/harness/mod.rs` holds the in-memory ports it drives.

`src/bin/reproduce.rs` is coarser and works end to end: five whole failure modes,
one PASS or FAIL line per behaviour, non-zero exit while any check fails.

Neither is the grader. The graded suite is broader, and several of its cases turn
on two or three of these rules interacting — see the list at the end of
`instruction.md`. Passing both aids is necessary, not sufficient.
