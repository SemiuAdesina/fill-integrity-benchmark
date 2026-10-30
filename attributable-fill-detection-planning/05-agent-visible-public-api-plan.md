# 05 — Agent-visible public API plan

Two categories: **frozen** (semantics asserted; changing them fails unrelated-behaviour
cases) and **open** (the agent may add, remove, or restructure freely).

The task must not prescribe *how* attribution is implemented. It prescribes only what is
observable.

## Frozen — types

```rust
pub struct MarketId(pub String);
pub struct TokenId(pub String);
pub struct OrderId(pub String);

pub enum Side { Yes, No }

pub struct Position {
    pub market_id: MarketId,
    pub token_id_yes: TokenId,
    pub token_id_no: TokenId,
    pub expiry_time: Timestamp,
    pub yes_order_id: OrderId,
    pub no_order_id: OrderId,
    pub size_yes: Qty,
    pub size_no: Qty,
    pub entry_yes_price: Price,
    pub entry_no_price: Price,
    pub entry_cost: Price,
    pub total_usd_cost: Money,
    pub created_at: Timestamp,
}
```

`Position::new` keeps the source arithmetic exactly:
`entry_cost = entry_yes_price + entry_no_price` and
`total_usd_cost = entry_yes_price * size_yes + entry_no_price * size_no`.

`Position::should_exit_now()` (60..=120 s window), `is_too_close_to_expiry()` (< 60 s),
`is_expired()`, `seconds_until_expiry()` — all frozen.

## Frozen — collections

```rust
impl PendingOrderManager {
    pub async fn add_order(&self, order: PendingOrder);
    pub async fn get_orders(&self) -> Vec<PendingOrder>;
    pub async fn get_orders_for_market(&self, market: &MarketId) -> Vec<PendingOrder>;
    pub async fn has_complete_set(&self, market: &MarketId) -> bool;
    pub async fn remove_order(&self, order_id: &OrderId);
    pub async fn get_orders_to_cancel(&self) -> Vec<PendingOrder>;
    pub async fn cleanup_expired(&self);
    pub async fn count(&self) -> usize;
    pub async fn get_all_markets(&self) -> Vec<MarketId>;
}

impl PositionManager {
    pub async fn add_position(&self, position: Position);
    pub async fn get_positions(&self) -> Vec<Position>;
    pub async fn get_positions_to_exit(&self) -> Vec<Position>;
    pub async fn remove_position(&self, market: &MarketId);
    pub async fn cleanup_expired(&self);
    pub async fn count(&self) -> usize;
}
```

`PendingOrder::should_cancel()` — 300 s before expiry, or past expiry — frozen.

Sizing is frozen verbatim:

```rust
pub fn calculate_minimum_order_size(price: Price, min_usd: Money) -> Qty;
pub fn calculate_kelly_order_size(
    yes_price: Price,
    no_price: Price,
    bankroll: Money,
    kelly_fraction: f64,
    max_position_pct: f64,
    min_order_value: Money,
    max_usd_per_market: Money,
) -> Qty;
```

## Frozen — entry point

```rust
pub async fn run_cycle(engine: &mut Engine) -> Result<CycleReport, EngineError>;
```

The verifier drives the world only through `run_cycle`. Its signature and the meaning of
one call — "advance the engine by one observation/decision round" — are frozen. The agent
may restructure everything inside it.

`CycleReport` is an observation record, not a control surface. It carries counts the
verifier can read (positions created this cycle, hedges submitted, cancels issued). It
must not become a channel through which the agent smuggles internal structure to the
tests; the verifier asserts on manager state and gateway history as the primary sources
and treats `CycleReport` as corroborating.

## Frozen — ports

The five trait definitions (docs 06–10) are frozen: the agent implements *against* them
and must not change their signatures, because the harness implements them. This is the one
place where the task does constrain shape, and only because the test double must plug in.

## Open — everything else

The agent may freely:

- add fields to `PendingOrder`, or replace it with a different internal representation, so
  long as the frozen `PendingOrderManager` methods keep returning coherent values
- introduce any attribution structure: ledgers, per-order cursors, claim tables, event
  logs, reconciliation passes
- add modules, traits, generics, newtypes
- change internal error types
- restructure `run_cycle`'s internals entirely
- choose any concurrency approach, or none

The task statement must not name a data structure, a helper function, or a field. It
states the invariants (doc 12) and lets the agent choose.

## Explicitly not frozen, and not hinted

The instruction must not mention: baselines, checkpoints, deltas, ledgers, claims,
idempotency keys, sequence numbers, or cursors. Every one of those is a solution
technique. The instruction describes only observable behaviour — what must be true of
positions, hedges, cancellations, tracking, and exposure under each situation.
