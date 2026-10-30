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
`OrderAccepted`, `OpenOrder`, `FillEvent`, `FillBatch`.

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

`src/types/` is frozen: `StrategyId`, `InstrumentId`, `OrderId`, `ClientRef`, `Venue`,
`Qty`, `Price`, `qty_eq`, `qty_at_least`, `QTY_EPSILON`, `Duration`,
`Timestamp`, `StrategyCandidate`, `PendingOrder`, `CANCEL_LEAD`, `Position`,
`EXIT_WINDOW_START_SECONDS`, `EXIT_WINDOW_END_SECONDS`, and `Cursor`.

Their fields, constructors, methods, constants, and arithmetic must keep the
behaviour they have today.

## Snapshot

`Snapshot` is the only frozen type you are expected to extend. It must keep:

- the existing public fields
- `#[derive(Debug, Clone, Default, PartialEq)]`

You may add new public fields to it. `StateStore::load` returns whatever the
matching `StateStore::save` wrote, so anything you add survives a restart.

## Everything else

`src/engine/fill_classifier.rs`, `src/engine/neutralization.rs`,
`src/engine/cycle.rs`, `src/engine/managers.rs`, `src/engine/sizing.rs` and the
re-exports in `src/engine/mod.rs` are free to change. Rename them, split them,
merge them, or delete them.
