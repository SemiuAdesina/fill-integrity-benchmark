
# Attributable Fill Reconciliation Across Restarts and Neutralization

The Rust project to repair is at `/app`. The deliverable is the corrected source
tree there, left in a state that builds and runs. No report, summary or notes
file is expected; only the source tree is graded.

## The service

The engine works *pairing mandates*. A mandate is an instruction to acquire
matched quantity of two related instruments across two execution venues, called
North and South. Holding both halves in equal size is inventory-neutral; holding
only one half is an open exposure the desk does not want. For each mandate the
engine rests one limit order per venue. When both legs are acquired it records
one completed pairing. When only one is acquired the mandate is left one-legged,
and the engine closes the residual exposure with a *neutralization order* on the
venue that did not fill.

## The problem

The engine already keeps per-order attribution state and already wires it
through the cycle, the snapshot and the restart path.
`/app/src/engine/fill_classifier.rs` holds one record per order the engine
submitted and folds in the three things the venue says about an order: per-order
fill events from the feed, block matches that name several orders at once, and
the filled quantity carried on the live order list.
`/app/src/engine/cycle.rs` drives that ledger, persists it, and restores it.

The structure is sound. The semantics are not. The engine records pairings it
should not, sends corrective orders it should not, and is wrong at the edges.
Some of what it does looks defensible in isolation and still disagrees with the
contract. The defects are in how observations are read and how derived
quantities are computed, not in how the state is stored.

Nothing in the source says which readings are wrong. The documents under
`/app/spec` are the authority, and where the code and those documents disagree,
the code is what changes.

## The contract

`/app/spec/venue-semantics.md` states what the venue and the feed guarantee:
exact integer quantities, the half-open attribution window, order identity and
identifier recycling, the open-order list, the at-least-once fill feed, and
block fills including the division rule in full.

`/app/spec/reconciliation-contract.md` states what the engine must do with those
observations: how attribution progress accumulates, block finality, residual
exposure and committed neutralizations, the closure sequence, submission
outcomes, and what has to survive a restart.

`/app/spec/frozen-contract.md` is the public surface to preserve, and names the
one frozen type you are expected to extend. `/app/spec/source-map.md` says where
each concern currently lives and where the code commits to one reading of the
venue; it is a map rather than a solution.

Read the first two before writing code.

## What has to be true

Quantity counts toward a leg only when it is genuinely attributable to an order
the engine submitted for that leg. Custody inventory aggregates across mandates,
across runs and across orders no longer tracked, so it can never prove that one
particular order filled. Two mandates may name the same instrument, and quantity
acquired by one must not complete the other.

Direct fill progress for an order is cumulative. The fill feed and the
open-order list report that same running total rather than two independent
fills, so repeated, replayed or stale observations move a high-water mark
instead of adding quantity. Shares derived from block fills are additive, and
are made idempotent by `block_ref`.

Every timestamped observation is tested against that order's own half-open
attribution window, against the venue's match timestamp rather than the cycle
that happened to read it. An observation exactly at the window start is inside
it; one exactly at the window end is outside it.

A block reports only a total, so the engine derives each covered order's share:
an eligible set, a basis of outstanding quantity, truncated integer shares, and
the base units left by truncation placed in the deterministic order the
specification states. Every tracked order the block covers is finalized
afterwards, including one the division allocated nothing to.

A one-legged mandate is corrected for its residual exposure only — the required
quantity, less what is already attributed to it, less what is still working in
neutralizations the engine has already committed to — so that consecutive cycles
cannot send the same exposure twice. A leg whose state is momentarily unknown is
never neutralized against, and a resting leg is moved into closure before a
corrective order is issued for whatever remains. A lost acknowledgement is
retried under the same client reference and must not place a second economic
order; a rejection creates nothing and leaves the exposure open to a later
attempt. Mandates are independent, and one that has already been corrected once
must stay able to make progress.

Across a restart, settled work stays settled. Original window starts, feed
position, attribution already accumulated, absorbed block references, tracked
work and committed neutralizations all survive it, and pre-existing inventory is
not re-read as new fills.

Quantities are whole base units throughout. There is no sub-unit quantity, no
comparison tolerance and no rounding, and where a quantity is divided the
division truncates toward zero. A single base unit lost or invented changes
which legs complete and the size of the corrective orders that follow.

## Scope

Everything under `/app/src` is yours, subject to `/app/spec/frozen-contract.md`.
Modules under `/app/src/engine/` may be added, split, renamed or removed, and
`Snapshot` is the one frozen type meant to grow. Nothing depends on a particular
helper, struct, field or algorithm: grading drives the engine only through its
frozen public surface and its injected ports, so any internally correct design
passes. Behaviour unrelated to fill processing should stay as it is. The task is
fully offline — no network calls, no added dependencies, no wall-clock sleeps,
and no custody account, key or venue credential of any kind.

## Checking your work

The graded suite runs outside this environment and is not present in `/app`. Two
offline aids are, and both are yours to extend, replace or ignore; neither is
graded.

```
cargo build --offline
cargo test --offline --test diagnostics
cargo run --offline --bin reproduce
```

`/app/tests/diagnostics.rs` checks one documented rule at a time on synthetic
values and reports the rule it broke rather than a case number, so a failing
line points at a sentence of the specification rather than at a fixture.
`/app/src/bin/reproduce.rs` is coarser and drives five whole failure modes end to
end against in-memory fakes, printing one line per behaviour.

Passing both is necessary rather than sufficient. Several graded situations turn
on two or three of these rules interacting, and no single-rule check can tell you
that a combination is right.
