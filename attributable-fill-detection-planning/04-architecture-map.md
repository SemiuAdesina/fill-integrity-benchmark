# 04 — Architecture map (sanitized task)

## Layering

```
                       ┌──────────────────────────────┐
   verifier ─────────► │  Harness (sim)               │
   drives              │  Schedule → FakeVenue        │
                       │  FakeClock, FakeStateStore   │
                       └───────────────┬──────────────┘
                                       │ implements
                       ┌───────────────▼──────────────┐
                       │  Ports (traits)              │
                       │  BalanceSource               │
                       │  OrderGateway                │
                       │  FillEventSource             │
                       │  StateStore                  │
                       │  Clock                       │
                       └───────────────┬──────────────┘
                                       │ consumed by
   ┌───────────────────────────────────▼──────────────────────────────┐
   │  Engine                                                          │
   │                                                                  │
   │   cycle::run_cycle(ports, managers, settings)                    │
   │     1. drain fill events        (FillEventSource)                │
   │     2. observe balances         (BalanceSource)                  │
   │     3. classify + attribute     ◄── DEFECT LIVES HERE            │
   │     4. create positions         (PositionManager)                │
   │     5. hedge unresolved         (OrderGateway)                   │
   │     6. cancel near-expiry       (OrderGateway)                   │
   │     7. cleanup expired tracking (PendingOrderManager)            │
   │     8. persist snapshot         (StateStore)                     │
   │                                                                  │
   │   PendingOrderManager   PositionManager                          │
   │   fishing::sizing                                                │
   └──────────────────────────────────────────────────────────────────┘
```

The engine has **no** direct dependency on any network, chain, filesystem, or wall clock.
Every external effect crosses a port. This is the property that makes the task offline and
deterministic.

## Cycle ordering (frozen)

`run_cycle` is the unit of virtual time. The verifier advances the world by calling it a
fixed number of times, so "eventual progress" is expressible as a bounded cycle count.

Step order matters and is fixed by the driver:

| Step | Reads | Writes |
|---|---|---|
| 1 drain fill events | `FillEventSource` | acknowledged fill state |
| 2 observe balances | `BalanceSource` | observed balance state |
| 3 classify + attribute | tracked orders, (1), (2) | per-order resolved/unresolved exposure |
| 4 create positions | (3) | `PositionManager`, `PendingOrderManager` |
| 5 hedge unresolved | (3) | `OrderGateway`, `PendingOrderManager` |
| 6 cancel near-expiry | `PendingOrderManager`, `Clock` | `OrderGateway`, `PendingOrderManager` |
| 7 cleanup expired | `PendingOrderManager`, `Clock` | `PendingOrderManager` |
| 8 persist snapshot | all engine state | `StateStore` |

Step 6 precedes step 7 — the source repository has these inverted, which is a separate
defect deliberately kept out of scope (allowlist decision D1).

Steps 3 and 5 are the agent's working area. Steps 1, 2, 4, 6, 7, 8 are scaffolding the
agent may reorganise but whose observable effects are asserted.

## Restart boundary

```
   cycles 1..k ──► StateStore::save(snapshot)
                          │
                   [process restart]
                          │
   cycles k+1.. ◄── StateStore::load() ──► engine state rebuilt
```

The harness models restart by dropping the engine value entirely and rebuilding it from
`StateStore::load()`, while the `FakeVenue` (balances, resting orders, fill cursor)
persists. This is exactly the real hazard: the wallet remembers, the process does not.

## Where the defect sits

Step 3 in the shipped state derives per-order completion from the absolute balance
returned by step 2. It ignores step 1 entirely — the source repository never consults any
per-order fill signal, and the shipped task preserves that. The `FillEventSource` port is
present and wired, and the harness emits well-formed events on it, but the shipped
classification code does not consume them.

That is the discoverable structure: an authoritative per-order signal is available and
unused, while a shared aggregate is being used as if it were per-order.

## Data flow of the self-amplifying loop (shipped behaviour)

```
  balance(t) ──► classify: "leg unfilled" ──► hedge BUY ──► balance(t+1) increases
       ▲                                                            │
       └────────────────────────────────────────────────────────────┘
```

The compensating action feeds the faulty detector. The loop terminates only when the
inflated leg crosses the other leg's threshold, so the damage is bounded by the size
ratio but is not self-correcting in any useful sense.
