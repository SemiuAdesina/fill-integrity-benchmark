# 10 — `Clock` design

Required. Three admitted files call `Utc::now()` directly:
`PendingOrder::new` (`created_at`), `PendingOrder::should_cancel`, and four `Position`
time predicates. Left alone, every expiry-related assertion would be wall-clock dependent
and the suite would flake.

## Trait

```rust
pub trait Clock: Send + Sync {
    fn now(&self) -> Timestamp;
}
```

## `FakeClock`

- starts at a fixed epoch constant (no `Utc::now()` anywhere in the task)
- advances **only** when the driver advances it, by a fixed step per cycle
- the schedule may request additional jumps between cycles, to cross the 300 s
  cancellation boundary or an expiry without running hundreds of cycles

```rust
pub struct FakeClock { current: AtomicI64 }

impl FakeClock {
    pub fn advance(&self, by: Duration);
    pub fn set(&self, to: Timestamp);
}
```

## Consequences

**No sleeping.** The engine holds no timer and cannot make time pass. `tokio::time::sleep`
against a paused runtime, or a busy-wait, does not advance `FakeClock` and does not change
any port's return value. A "sleep until balances settle" strategy therefore observes the
same state forever and misses the deadline in I13 — this is what makes partial fix P6 fail
by behaviour rather than by a source scan for the word `sleep`.

**Expiry is exact.** `should_cancel()`'s 300 s rule and `should_exit_now()`'s 60..=120 s
window become precisely reachable, so the frozen-behaviour regression cases are exact
rather than approximate.

**Reproducible.** Two runs of the same schedule produce identical timestamps, so
`created_at` on every `PendingOrder` and `Position` is stable and directly assertable.

## Scope note

`Timestamp` is a thin newtype over an integer epoch-millisecond value rather than
`chrono::DateTime<Utc>`. This drops the `chrono` dependency (doc 17), removes any locale
or leap-second surface, and keeps the arithmetic in the admitted predicates identical.
`Duration` is likewise a plain integer-millisecond newtype.
