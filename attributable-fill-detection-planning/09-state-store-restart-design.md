# 09 — `StateStore` and restart-snapshot design

The source repository's main binary keeps **all** state in memory
(`Arc<RwLock<Vec<PendingOrder>>>`, `Arc<RwLock<Vec<Position>>>`) and persists nothing. On
restart every tracked order is forgotten while the wallet still holds the tokens those
orders produced. Every live order becomes a permanent orphan, and every historical balance
becomes indistinguishable from a fresh fill.

The task makes that hazard testable by introducing persistence as a port.

## Trait

```rust
#[async_trait]
pub trait StateStore: Send + Sync {
    async fn save(&self, snapshot: &Snapshot) -> Result<(), PortError>;
    async fn load(&self) -> Result<Option<Snapshot>, PortError>;
}
```

## `Snapshot`

The shape is **deliberately owned by the agent**. The task ships a `Snapshot` that is
serde-serialisable and contains what the shipped engine happens to need; the agent is free
to extend it, and will have to.

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Snapshot {
    pub schema_version: u32,
    pub tracked_orders: Vec<PendingOrder>,
    pub positions: Vec<Position>,
    pub fill_cursor: Cursor,
}
```

The shipped snapshot is *insufficient* for correct restart behaviour, and that
insufficiency is part of the defect rather than a trick: it records which orders exist but
nothing about how much of each has been acknowledged as filled, and nothing that
distinguishes wallet inventory the engine caused from inventory it did not. After a
restart the shipped engine reads a large balance, finds a tracked order, and concludes the
order just filled.

Extending `Snapshot` is expected. `schema_version` exists so the agent can change the
shape without the harness rejecting it — the harness round-trips whatever the engine
writes and never validates internal fields. It asserts only on *behaviour after reload*.

## Restart modelling

`FakeStateStore` holds one serialised blob in memory. Restart is modelled by the harness as:

1. run cycles `1..k`, during which the engine saves each cycle (driver step 8)
2. **drop the `Engine` value entirely**
3. construct a fresh `Engine` over the *same* `FakeVenue`, `FakeClock`, and
   `FakeStateStore`
4. the new engine calls `load()` during construction
5. run cycles `k+1..n`

The venue keeps its balances, its resting orders, and its fill-event log. The engine keeps
only what it wrote. This is the real asymmetry, reproduced exactly.

## Restart hazards the design exposes

| Hazard | How the schedule produces it |
|---|---|
| Historical balance reinterpreted as a new fill | Restart with a large non-zero balance and a tracked order still open (V11) |
| Fill events replayed from the beginning | `fill_cursor` not persisted, or persisted stale → the engine re-reads events it already acted on |
| Fill events skipped | `fill_cursor` persisted too far ahead → a genuine fill is never seen |
| Live order forgotten | Tracked order absent from the snapshot → orphan (I11 violation) |
| Lost-ack hedge forgotten | Submission accepted, ack lost, restart before any record → the engine resubmits (V9 crossed with V11) |

The cursor hazards cut in both directions on purpose. There is no single safe constant; the
agent must persist the cursor consistently with whatever it treats as acknowledged.

## Determinism

In-memory blob, no filesystem, no clock, no encoding surprises. `save` is total; the
harness can also be configured to fail `save` on a given cycle so the agent cannot assume
persistence always succeeds — a snapshot written for cycle *k* may not exist after a
restart at *k+1*, and the engine must still not double-hedge.
