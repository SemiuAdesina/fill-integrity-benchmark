# 08 — `FillEventSource` design

Replaces the CLOB user WebSocket (`src/clients/poly_api/websocket.rs`) and
`src/app/ws_fill_listener.rs`.

This port is the authoritative per-order signal that the source repository has available
and never uses. Its presence is what makes the task solvable; the shipped engine ignoring
it is what makes the task a bug.

## Trait

```rust
#[async_trait]
pub trait FillEventSource: Send + Sync {
    async fn poll(&self, from: Cursor) -> Result<FillBatch, PortError>;
}

pub struct FillBatch {
    pub events: Vec<FillEvent>,
    pub next: Cursor,
}

pub struct FillEvent {
    pub seq: u64,
    pub order_id: OrderId,
    pub market_id: MarketId,
    pub token_id: TokenId,
    pub side: Side,
    pub filled_qty_delta: Qty,
    pub cumulative_filled_qty: Qty,
}
```

## Guarantees

1. **Per-order.** Every event names the `order_id` it belongs to. Attribution is therefore
   *possible* — unlike a balance read.
2. **Cumulative and monotonic per order.** `cumulative_filled_qty` never decreases for a
   given `order_id`. This lets a correct solution be idempotent without inventing its own
   sequencing.
3. **At-least-once, not exactly-once.** The harness may redeliver an event the engine has
   already seen (V4). Redelivery repeats the same `seq` and the same
   `cumulative_filled_qty`. A solution that accumulates `filled_qty_delta` without regard
   to `seq` or cumulative double-counts and fails.
4. **Ordered by `seq` within a batch**, but a batch may contain events for several markets
   and several orders interleaved.
5. **Delayed, never invented.** The harness may withhold an event for a configured number
   of cycles (V7), then deliver it. It never fabricates a fill that did not occur at the
   venue, and never omits one permanently.
6. **Covers every order the engine created**, including hedge orders and — critically —
   orders created by a submission whose acknowledgement was lost. That is how a correct
   solution discovers that the lost-ack order is live.

## What the port does not do

- It does not tell the engine whether a pair is complete.
- It does not compute exposure.
- It does not reconcile against balances.
- It does not deduplicate on the engine's behalf.

Those are the agent's work. The port supplies attributable raw material and nothing more.

## Relationship to `BalanceSource`

Deliberately overlapping and deliberately inconsistent in ways the engine must survive:

| Situation | `FillEventSource` | `BalanceSource` |
|---|---|---|
| Historical inventory from before tracking | silent | included in the total |
| Tracked resting order fills | event with `order_id` | total rises |
| Hedge order fills | event with the hedge's `order_id` | total rises |
| Two markets sharing a token | two distinct events, distinct `order_id` | one indistinguishable total |
| Redelivered event | duplicate `seq` | unchanged |
| Withheld event | nothing yet | may already reflect the fill |
| Merge / redemption outflow | no event | total falls |

Reading the table across a row is the task. Balance alone collapses every row into one
number; events alone miss the last row. A robust solution uses events as the attribution
authority and balances as a consistency check — but the task states neither. It states the
invariants and lets the agent discover the relationship.

## Determinism

Delivery is a pure function of cycle index and schedule. Cursors are opaque `u64`. No
timers, no channels, no background tasks — `poll` is called synchronously inside
`run_cycle`, so there is no scheduler nondeterminism to flake on.
