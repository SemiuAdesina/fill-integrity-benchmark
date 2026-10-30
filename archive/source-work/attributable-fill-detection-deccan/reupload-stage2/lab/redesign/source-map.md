# Source map

Where things are, so that finding them is not the exercise. What the engine must
*do* is in `venue-semantics.md`, `reconciliation-contract.md` and
`frozen-contract.md`; this file only says where each concern currently lives.

## Entry point

`Engine::run_cycle` in `src/engine/cycle.rs` is the only thing the graded suite
calls. Every behaviour below is reached from it.

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
| Legs are judged complete, positions are created | `engine/cycle.rs` — `reconcile_open_strategies`, `open_position` |
| Corrective orders are sized, priced and sent | `engine/cycle.rs` — `close_gap`; `engine/neutralization.rs`, `engine/sizing.rs` |
| Corrective orders already committed to | `engine/cycle.rs` — `reconcile_neutralizations`; `NeutralizationRecord` |
| Custody inventory is read | `ports/mod.rs` — `InventorySource` (affordability) |
| Orders being tracked, positions held | `engine/managers.rs` — `PendingOrderManager`, `PositionManager` |
| What survives a restart | `types/snapshot.rs` — `Snapshot`, via `StateStore`; `engine/cycle.rs` — `ensure_loaded`, `persist` |
| The attribution interval | `types/pending_order.rs` — `attribution_start`, `attribution_end`, `ATTRIBUTION_WINDOW` |
| Exact integer division | `types/ids.rs` — `proportional_share`, `Qty`, `QTY_UNIT` |

## Files you are expected to change

Under `src/engine/` and `src/types/snapshot.rs`. `frozen-contract.md` is
authoritative on what is frozen; in short, the engine internals are yours and
`Snapshot` may gain fields.

You may reorganise those internals as you like: add modules under `src/engine/`,
split a file into a subdirectory, rename, or leave the layout exactly as it is.
`src/lib.rs` must still expose the frozen surface. Nothing is graded on file
layout.

Not to be changed: `src/ports/mod.rs` and the rest of `src/types/`. Both are
frozen, and the graded suite drives the engine through them.

Everything under `tests/` is diagnostics: six suites and the in-memory venue in
`tests/harness/` they drive the engine against. None of it is graded, and none
of it is read when your work is scored. Edit it freely.

## Shared instruments

`StrategyCandidate` carries an `InstrumentId` per venue and two mandates may
carry the same one. `instrument_inventory` aggregates across mandates, across
runs, and across orders no longer tracked, so it cannot separate them.

## Checking your work

```sh
cargo build --offline
cargo test --offline --test observation_identity
cargo test --offline --test direct_progress
cargo test --offline --test block_reconciliation
cargo test --offline --test neutralization_lifecycle
cargo test --offline --test restart_recovery
cargo test --offline --test cross_mandate_isolation
```

Each suite runs on its own and reports the observable outcome it saw. The graded
suite is not here; it is broader and holds the engine to the whole contract.
