# 18 — Rust crate and module plan

Layout of the sanitized crate. Paths below are relative to the crate root inside the
package; the *package* directory structure (where the crate sits, what sits beside it) is
owned by the Official Contributor Guidelines and is not decided here.

## Single crate, binary-less

One library crate with an integration test suite. No `[[bin]]` — the source repository's
five binaries all reach the network. Nothing in the task needs a runnable entry point; the
harness drives `run_cycle` directly.

```
Cargo.toml
Cargo.lock
rust-toolchain.toml
.cargo/config.toml
src/
  lib.rs
  types/
    mod.rs
    ids.rs
    pending_order.rs
    position.rs
    time.rs
  ports/
    mod.rs
    balance_source.rs
    order_gateway.rs
    fill_event_source.rs
    state_store.rs
    clock.rs
    error.rs
  engine/
    mod.rs
    pending_orders.rs
    positions.rs
    fill_classifier.rs
    hedging.rs
    cycle.rs
    sizing.rs
    config.rs
tests/
  common/
    mod.rs
    fake_venue.rs
    fake_clock.rs
    fake_state_store.rs
    schedule.rs
    driver.rs
    observations.rs
    generator.rs
  scenarios.rs
```

## Module responsibilities

**`types/ids.rs`** — `MarketId`, `TokenId`, `OrderId`, `ClientRef`, `Side`, `Qty`, `Price`.
Newtypes over `String` / `f64`, `Ord` where needed so collections iterate deterministically.

**`types/time.rs`** — `Timestamp`, `Duration` as integer-millisecond newtypes (doc 10).

**`types/pending_order.rs`** — mapped from source A1. `market_question` dropped (D3).
`created_at` supplied by the caller rather than `Utc::now()`. `should_cancel()` takes the
current time as a parameter. The 300 s rule is unchanged and frozen.

**`types/position.rs`** — mapped from source A2. `Position::new` arithmetic frozen
(`entry_cost`, `total_usd_cost`); four time predicates frozen, each taking the current time
as a parameter.

**`ports/*`** — the five traits from docs 06–10 plus `PortError` / `SubmitError`. Traits
only: no implementation lives here, and the crate ships no real adapter. There is nothing in
the library that could reach a network even if a solution tried.

**`engine/pending_orders.rs`** — `PendingOrderManager`, nine frozen public methods
(source A3). Backed by an ordered collection so iteration is deterministic — this is a
change from the source's `Vec` behind `Arc<RwLock<_>>` only in that ordering is now
guaranteed.

**`engine/positions.rs`** — `PositionManager`, six frozen public methods (source A4). All
`println!` calls removed.

**`engine/fill_classifier.rs`** — **the defect site.** Mapped from source A5
(`filled_order_checker.rs`), with the per-leg threshold pre-corrected (doc 15) and the
pairing logic retained. This is where the agent will spend its time.

**`engine/hedging.rs`** — `attempt_orphan_hedge` and `ORPHAN_HEDGE_MAX_COMBINED`, split out
of A5 so the classifier file stays readable. Ships with full-leg hedging and fresh-
`ClientRef`-per-attempt (P8 and P4 origins).

**`engine/cycle.rs`** — `run_cycle(&mut Engine) -> Result<CycleReport, EngineError>` and the
`Engine` struct that owns the five ports and the two managers. The frozen 8-step order from
doc 04. `CycleReport` is the engine's own observable summary; the harness also observes the
managers and the gateway log directly, so `CycleReport` is not the only assertion surface
and a solution cannot game it.

**`engine/sizing.rs`** — `calculate_minimum_order_size` and the Kelly path from source A6,
both frozen.

**`engine/config.rs`** — the six retained settings (doc 02): `min_order_value`,
`orphan_hedge_max_combined`, `cancel_lead_seconds`, `max_pending_orders`, `fill_tolerance`,
`hedge_retry_limit`. Plain struct with a `Default`. No `dotenv`, no environment reads, no
file loading.

## Test-side modules

Everything under `tests/common/` is harness code. The agent may read it — it is not hidden
— but the graded suite lives elsewhere per the hidden-test posture, which is blocked on the
official guide.

**`fake_venue.rs`** — implements `BalanceSource`, `OrderGateway`, and `FillEventSource` over
one shared state. Owns the append-only submission log (doc 07) and the fill-event log
(doc 08). Implements every `Action` variant's effect.

**`driver.rs`** — the loop from doc 11. One `run_cycle` per cycle, restart handling, per-
cycle observation capture.

**`generator.rs`** — V13's bounded-grammar generator and the small deterministic PRNG
(doc 17). Computes expected outcomes independently of the engine.

**`scenarios.rs`** — V1–V13 as `#[tokio::test(flavor = "current_thread")]` functions.

## Boundary rules

1. **Nothing in `src/` depends on anything in `tests/`.** The library has no test-only
   feature flag, no `cfg(test)` branch that changes behaviour, and no way to detect being
   under test.
2. **Nothing in `src/` performs I/O.** No filesystem, no network, no clock, no stdout. Every
   external effect goes through a port.
3. **No `unsafe`.** `#![forbid(unsafe_code)]` at the crate root.
4. **No comments anywhere in shipped source**, per the clean-code policy. Names carry the
   meaning: `acknowledged_filled_qty`, `unresolved_exposure`, `attribution_checkpoint` are
   the kind of names used — though note the *shipped* code must not name concepts it does
   not yet have (doc 05 forbids hinting at baselines or checkpoints in the starting state).
5. **Determinism by construction.** Ordered collections, single-threaded runtime, no
   entropy, no wall clock.

## What the agent is free to restructure

Everything except the frozen surface in doc 05. New modules, new types, new fields on
`Snapshot`, different internal collections — all fine. The verifier asserts on manager state
and gateway history, so a solution that reorganises `engine/` entirely still passes if it
behaves correctly.

## Line-count estimate

| Area | Approx. lines |
|---|---|
| `types/` | 220 |
| `ports/` | 180 |
| `engine/` | 520 |
| `tests/common/` | 700 |
| `tests/scenarios.rs` | 600 |

Around 2,200 lines total, roughly 900 of which the agent reads as the system under test.
Large enough to be a real system, small enough to hold in context.
