# Frozen public contract

The graded verification drives this crate only through the items listed here.
You may restructure everything else, including the internals of `src/engine/`,
the modules under it, and any helper functions.

## Library entry points

`pairing_engine::Engine` must keep this shape:

```rust
impl<'a> Engine<'a> {
    pub fn new(
        config: EngineConfig,
        strategies: &'a dyn StrategySource,
        inventory: &'a dyn InventorySource,
        gateway: &'a dyn OrderGateway,
        fills: &'a dyn FillEventSource,
        store: &'a dyn StateStore,
        clock: &'a dyn Clock,
    ) -> Self;

    pub fn run_cycle(&mut self) -> Result<(), PortError>;
    pub fn positions(&self) -> Vec<Position>;
    pub fn pending_orders(&self) -> Vec<PendingOrder>;
}
```

`EngineConfig` keeps its public fields and its `Default` values.

## Ports

The six traits in `src/ports/mod.rs` are frozen, together with the data types
they carry: `PortError`, `SubmitError`, `OrderKind`, `OrderRequest`,
`OrderAccepted`, `OpenOrder`, `FillEvent`, `BlockFill`, `FillBatch`.

```
InventorySource    instrument_inventory, settlement_balance
OrderGateway     submit, cancel, open_orders, best_offer
StrategySource     candidates
FillEventSource  poll
StateStore       load, save
Clock            now
```

Every port method takes `&self` and returns a `Result`. No port may be made
asynchronous and no port may be removed from `Engine::new`.

## Value types

`src/types/` is frozen: `StrategyId`, `InstrumentId`, `OrderId`, `ClientRef`,
`BlockRef`, `Venue`, `Qty`, `Price`, `qty_eq`, `qty_at_least`, `QTY_UNIT`,
`proportional_share`, `Duration`, `Timestamp`, `StrategyCandidate`,
`PendingOrder`, `CANCEL_LEAD`, `ATTRIBUTION_WINDOW`, `Position`,
`EXIT_WINDOW_START_SECONDS`, `EXIT_WINDOW_END_SECONDS`, and `Cursor`.

Their fields, constructors, methods, constants, and arithmetic must keep the
behaviour they have today. In particular:

- `Qty` stays an integer type. Quantity comparison stays exact, and quantity
  division stays truncated toward zero. Do not reintroduce a floating point
  quantity, a comparison epsilon, or a rounding step.
- `ATTRIBUTION_WINDOW` keeps its value, and `PendingOrder::attribution_start`
  and `PendingOrder::attribution_end` keep describing a half-open window.
- `proportional_share` keeps the exact integer formula documented on it.

## Snapshot

`Snapshot` is the only frozen type you are expected to extend. It must keep:

- its name and its position in `StateStore::load` and `StateStore::save`
- `#[derive(Debug, Clone, Default, PartialEq)]`

Its fields are yours: add to them, rename them, or restructure them. The graded
suite never reads a field of `Snapshot` — it only lets the engine save one and
hands the same one back on a later run, which is what a restart is. So whatever
the engine needs across a restart, it needs to be in here.

Two of the diagnostics under `tests/` do read fields, to check that durable state
is durable. If you restructure, adjust those; they are yours too.

## Everything else

`src/engine/fill_classifier.rs`, `src/engine/neutralization.rs`,
`src/engine/cycle.rs`, `src/engine/managers.rs`, `src/engine/sizing.rs` and the
re-exports in `src/engine/mod.rs` are free to change. Rename them, split them,
merge them, or delete them. You may add modules and subdirectories under
`src/engine/`, and there is no requirement that the work stay in one file — only
that `src/lib.rs` still exposes the surface above.

The types `Snapshot` is built out of — `SubmissionRecord` and
`NeutralizationRecord` in `src/types/snapshot.rs` — are part of the engine's own
bookkeeping rather than the frozen surface, and are free to change with it.
