# Source map

Where things are, so that finding them is not the exercise. What the engine must
*do* is in `frozen-contract.md` and `venue-semantics.md`; this file only says
where each concern currently lives.

Nothing here is a solution sketch. The repair itself — what attribution should be
built from, and how — is the task.

## Entry point

`Engine::run_cycle` in `src/engine/cycle.rs` is the only thing the graded suite
calls. Every behaviour below is reached from it. Read it first: at 270 lines it
is the whole control flow, and the rest of the crate is types and helpers it
calls.

One cycle, in the order it runs today:

```
run_cycle
├── ensure_loaded            restore Snapshot on first cycle       cycle.rs
├── refresh_fill_stream      poll the fill feed                    cycle.rs
├── enter_new_strategies     size and submit both legs             cycle.rs → sizing.rs
├── reconcile_open_strategies decide what filled, and act          cycle.rs → fill_classifier.rs
│   └── neutralize_missing_leg                                     cycle.rs → neutralization.rs
├── retire_stale_orders      cancel near expiry                    cycle.rs → managers.rs
└── persist                  write Snapshot back                   cycle.rs
```

## Which concern lives where

| Concern | File |
|---|---|
| Fill observations arrive | `ports/mod.rs` — `FillEventSource::poll`, returning `FillBatch` |
| Fill observations are consumed | `engine/cycle.rs` — `refresh_fill_stream` |
| A tracked order is judged filled | `engine/fill_classifier.rs` — `classify_legs` |
| Per-order attribution state | nowhere — no such state exists today |
| Custody inventory is read | `ports/mod.rs` — `InventorySource::instrument_inventory` |
| A position is created | `engine/cycle.rs` — `open_position` |
| A corrective order is planned | `engine/neutralization.rs` — `plan_neutralization` |
| A corrective order is sent | `engine/cycle.rs` — `neutralize_missing_leg` |
| Orders being tracked | `engine/managers.rs` — `PendingOrderManager` |
| Positions held | `engine/managers.rs` — `PositionManager` |
| What survives a restart | `types/snapshot.rs` — `Snapshot`, via `StateStore` |
| Restart restore path | `engine/cycle.rs` — `ensure_loaded`, and `persist` |
| The attribution interval | `types/pending_order.rs` — `attribution_start`, `attribution_end`, `ATTRIBUTION_WINDOW` |
| Exact integer division | `types/ids.rs` — `proportional_share`, `Qty`, `QTY_UNIT` |
| Feed position across cycles | `types/snapshot.rs` — `Cursor` |

## Where the state you need does not yet exist

Three of the rows above are the shape of the defect rather than a location.

`refresh_fill_stream` polls the feed from `Cursor::beginning()` every cycle and
keeps only `batch.events.len()`. The events themselves — which carry
`order_id`, `client_ref`, `cumulative_filled_qty` and the match timestamp `at` —
are dropped, and so is `next_cursor`. Nothing in the crate reads `BlockFill`.

`classify_legs` answers "did this order fill?" from
`InventorySource::instrument_inventory`, which is per *instrument* and not per
order. There is no per-order state anywhere for it to consult instead.

`Snapshot` carries `positions`, `pending_orders` and `attempted_strategies`.
Whatever per-order attribution state you introduce is not in it, and
`StateStore::load` returns exactly what `save` wrote. `frozen-contract.md`
records that `Snapshot` is the one frozen type you are expected to extend.

## Files you are expected to change

Under `src/engine/` and `src/types/snapshot.rs`. `frozen-contract.md` is
authoritative on what is frozen; in short, the engine internals are yours and
`Snapshot` may gain fields.

Not to be changed: `src/ports/mod.rs` and the rest of `src/types/`. Both are
frozen, and the graded suite drives the engine through them.

`src/bin/reproduce.rs` is a diagnostic, is not graded, and is discarded before
grading. Edit it freely.

## Shared instruments

`StrategyCandidate` carries an `InstrumentId` per venue and two mandates may
carry the same one. `instrument_inventory` aggregates across mandates, across
runs, and across orders no longer tracked, so it cannot separate them. The
reproducer's second scenario demonstrates exactly this.

## Checking your work

```sh
cargo build --offline
cargo run --offline --bin reproduce
```

The reproducer drives five failure modes with in-memory fakes and prints one PASS
or FAIL line per behaviour, exiting non-zero while any check fails. It covers
five of them, and the graded suite is broader — see the list at the end of
`instruction.md`. It deliberately leaves the exact window, division, tie-break
and idempotency rules to the grader. Passing the reproducer is necessary, not
sufficient.
