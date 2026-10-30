
# Attributable Fill Reconciliation Across Restarts and Neutralization

The Rust project to repair is at `/app`. The deliverable is the corrected source
tree there, left in a state that builds. No report, summary or notes file is
expected; only the source tree is graded.

## The service

The engine works *pairing mandates*. A mandate is an instruction to acquire
matched quantity of two related instruments across two execution venues, called
North and South. Holding both halves in equal size is inventory-neutral; holding
only one half is an open exposure the desk does not want. For each mandate the
engine rests one limit order per venue. When both legs are acquired it records
one completed pairing. When only one is acquired the mandate is left one-legged,
and the engine closes the residual exposure with a *neutralization order* on the
venue that did not fill.

The engine decides all of this from what the venue tells it: per-order fill
events from a feed, block matches that name several orders at once, the live
order list, the outcome of each submission, and the state it persisted before a
restart. As shipped, the engine records pairings it should not, sends corrective
orders it should not, and leaves exposure open that it should close.

## The contract

`/app/spec/venue-semantics.md` states what the venue and the feed guarantee.
`/app/spec/reconciliation-contract.md` states what the engine must do with those
observations. Both are normative, and where the code and those documents
disagree, the code is what changes. `/app/spec/frozen-contract.md` is the public
surface to preserve. `/app/spec/source-map.md` says where each concern currently
lives.

## Scope

Everything under `/app/src` is yours, subject to `/app/spec/frozen-contract.md`.
The engine internals under `/app/src/engine/` may be restructured freely, and
`Snapshot` in `/app/src/types/snapshot.rs` may grow. Grading drives the engine
only through its frozen public surface and its six injected ports, so any
internally correct design passes. Behaviour unrelated to fill processing should
stay as it is. The task is fully offline: no network calls, no added
dependencies, no wall-clock sleeps, and no custody account, key or venue
credential of any kind.

## Building and checking

```
cargo build --offline
cargo test --offline --test observation_identity
cargo test --offline --test direct_progress
cargo test --offline --test block_reconciliation
cargo test --offline --test neutralization_lifecycle
cargo test --offline --test restart_recovery
cargo test --offline --test cross_mandate_isolation
```

The six diagnostic suites under `/app/tests` drive the engine through its public
surface against an in-memory venue and report what they observed. They are
yours to extend, replace or ignore, and none of them is graded. The graded
suite runs outside this environment and holds the engine to the whole contract.
