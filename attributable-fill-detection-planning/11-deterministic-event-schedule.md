# 11 — Deterministic event-schedule representation

One declarative value defines a whole scenario. The harness interprets it; the engine never
sees it.

## Representation

```rust
pub struct Schedule {
    pub seed: u64,
    pub initial_balances: Vec<(TokenId, Qty)>,
    pub initial_tracked: Vec<TrackedOrderSeed>,
    pub steps: Vec<Step>,
}

pub struct Step {
    pub cycle: u32,
    pub actions: Vec<Action>,
}

pub enum Action {
    FillRestingOrder { order_ref: OrderRef, qty: Qty },
    CreditToken { token: TokenId, qty: Qty },
    DebitToken { token: TokenId, qty: Qty },
    RedeliverFillEvent { seq: u64 },
    WithholdFillEvents { cycles: u32 },
    FreezeBalanceReads { cycles: u32 },
    FailBalanceRead { token: TokenId },
    LoseAcknowledgement { next_submissions: u32 },
    RejectSubmission { next_submissions: u32 },
    FailCancel { order_ref: OrderRef },
    FailStateSave,
    AdvanceClock { by_ms: i64 },
    Restart,
}
```

`OrderRef` names an order by *role* (`Resting(market, side)`, `HedgeFor(market, side)`)
rather than by id, because ids are produced by the engine at runtime. This keeps schedules
readable and, importantly, keeps fixture ids out of the assertions — a solution cannot key
off a literal id it saw in a test (partial fix P10).

## Driver

```
for cycle in 1..=schedule.max_cycle:
    apply all actions scheduled for this cycle to FakeVenue / FakeClock / FakeStateStore
    if a Restart action fired: drop Engine, rebuild from StateStore
    run_cycle(&mut engine)
    record observations
assert
```

`run_cycle` is called exactly once per cycle. The engine cannot skip ahead, and cannot make
progress without being driven. "Within N cycles" is therefore an exact, non-flaky
liveness bound.

## Determinism guarantees

1. Single-threaded `current_thread` runtime. No task spawning inside the engine is
   required, and none is performed by the harness.
2. No wall clock anywhere — `FakeClock` only (doc 10).
3. No `Math.random`-equivalent: the `seed` drives a small deterministic PRNG used **only**
   by the harness to generate V13's schedules, never by the engine.
4. Iteration order of any harness-side map is sorted before use, so `HashMap` ordering
   cannot leak into behaviour.
5. `f64` comparisons in assertions use an explicit tolerance constant; quantities are
   chosen to be exactly representable (integers and simple decimals) so tolerance is a
   safety net, not a crutch.

## Seeded randomised schedules (V13)

The generator draws from a bounded grammar rather than arbitrary noise:

- 1–3 markets, optionally sharing a token id
- leg sizes drawn from the real sizer's output for a set of price pairs
- each leg fills fully, partially, or not at all
- 0–2 lost acknowledgements
- 0–3 event redeliveries
- 0–2 balance freezes of 1–3 cycles
- 0–1 restart at a random cycle
- 0–1 `FailStateSave`

Every generated schedule is checked by construction to have a well-defined expected
outcome — the generator computes the true fill state itself, independently of the engine,
and derives expectations from that. The engine is never consulted to decide what is
correct.

Seeds are fixed constants in the suite, not drawn from time or environment. A failing seed
reproduces exactly.

## Failure diagnostics

On assertion failure the harness prints the schedule, the per-cycle observation log, the
gateway submission log, and the balance-read log. Enough to reconstruct the run without
re-instrumenting anything.
