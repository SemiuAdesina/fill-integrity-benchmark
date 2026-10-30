# Reference plan

## Before

The starting repository is not greenfield. `src/engine/fill_classifier.rs` holds an
`AttributionLedger` with one `SubmissionRecord` per submitted order keyed by `ClientRef`, owns
the feed cursor and the set of absorbed `block_ref` values, reads `FillEvent::at`,
`FillBatch::blocks` and `venue_seq`, and divides a block with integer truncation.
`src/engine/cycle.rs` folds `open_orders` and the fill feed into that ledger every cycle,
derives three state leg visibility, keys neutralizations by `ClientRef`, sizes them from a gap,
and persists the whole structure. All of it compiles untouched.

What is wrong is spread across the readings, the derived quantities, the handling of submission
outcomes and the meaning of durable state under identifier recycling. Each wrong reading carries a
plausible justification in a comment, and none of the comments says it is wrong.

In `src/engine/fill_classifier.rs`:

1. `in_window` closes the window at both ends. The window is half-open.
2. `absorb_event` folds a cumulative quantity in as an increment. The feed is at least once and
   redelivers, so the record has to be a high-water mark.
3. `locate` resolves the venue identifier before the client reference, and learns an identifier
   from any observation. The venue recycles identifiers, so an observation naming a recycled one
   lands on the old record and is dropped by its window, or worse relabels it; the client reference
   is the key the engine always knows.
4. The eligible set is sorted by `order_id` alone. The identifier is the third key, not the first.
5. The truncation remainder is handed to the largest outstanding order. The rule places it one base
   unit at a time on the earliest orders in the documented ordering.
6. An order an earlier block has closed stays in a later block's eligible set. A closed order
   contributes no outstanding quantity to a later basis.

In `src/engine/cycle.rs`:

7. `close_gap` treats `LegVisibility::Unconfirmed` like `Withdrawn` and falls through to
   neutralization. Absence is not proof of withdrawal: the order may have filled. And a leg the
   venue closed by naming it in a block is treated as unconfirmed, so its real gap is never covered.
8. The gap is `leg.size - self.acquired_for(leg)`, which ignores quantity already working at the
   venue but not yet attributed, so consecutive cycles send the same exposure twice.
9. A lost submission acknowledgement drops the record entirely, for a corrective order and for an
   entry leg alike. The venue may have accepted an order it never named; the record has to be kept
   with no `order_id` so the retry reuses the same `ClientRef`, and an entry leg has to stay
   recoverable across a restart.
10. When a mandate settles, its closure and cancellation entries, keyed by venue identifier, are
    left behind, so a later order that wears a recycled identifier inherits them.

Families the starting repository gets right, and which a rewrite can easily break: the
`absorbed_blocks` dedup guard on replayed blocks, the outstanding-quantity basis of the division,
the `max` fold of `open_orders.filled_qty`, and the durability of window starts, absorbed block
references and the client reference counter.

## After

Three files change: `src/engine/fill_classifier.rs`, `src/engine/cycle.rs` and
`src/types/snapshot.rs`. `Engine::new` keeps its seven arguments, `run_cycle`, `positions` and
`pending_orders` keep their signatures, and no port changes.

```
solution/fixed/src/types/snapshot.rs           (changed: `EntryAttempt`, `Snapshot::entry_attempts`)
solution/fixed/src/engine/fill_classifier.rs   (changed)
solution/fixed/src/engine/neutralization.rs    (identical to the starter)
solution/fixed/src/engine/cycle.rs             (changed)
solution/fixed/src/engine/mod.rs               (identical to the starter)
```

`solution/solve.sh` copies each one to the matching path under /app, runs `cargo fmt` when
rustfmt is present, and builds offline.

### 1. `src/engine/fill_classifier.rs` — attribution ledger

- Fill events keep a high-water mark; block shares are added and deduplicated by `block_ref`;
  `open_orders.filled_qty` raises the same high-water mark. A record a block has closed acquires
  nothing more from any later observation.
- Every timestamped observation is tested against the record's half-open window,
  `at >= window_start && at < window_start + ATTRIBUTION_WINDOW`, at the venue's match timestamp.
- `locate` tries the client reference first and the venue identifier second, resolved at the
  observation's own instant: among the records that have worn that identifier, the one whose
  window contains the instant. `bind_order_id` learns an identifier only when the record has none.
- `absorb_block` resolves each covered identifier at the block's instant, deduplicates `covers`,
  builds the eligible set from records that are in window, not closed, and have outstanding
  quantity, sorts by `(window_start, venue_seq, order_id)` with an unknown `venue_seq` last,
  divides `min(block_qty, basis)` by truncated proportional shares, hands the remainder out one
  unit at a time in that order, and closes every covered record.

### 2. `src/engine/cycle.rs` — reconcile before acting

- **Completion.** Attributed quantity on the leg's own reference plus on its corrective orders,
  compared with exact integer `>=`. Custody inventory is never consulted for completion.
- **Visibility.** `Resting` when listed; `Withdrawn` when this engine cancelled it or the ledger
  reports it closed by a block; `Unconfirmed` otherwise, and never neutralized against. A leg the
  venue closed is covered directly, with no cancellation.
- **Residual.** `leg.size - acquired - working`, where an acknowledged corrective contributes its
  outstanding quantity, an unacknowledged one its full size, and a closed one nothing.
- **Submission outcomes.** A corrective order whose acknowledgement was lost keeps its record and
  is tracked under its reference; the next cycle resubmits under the same reference and the venue's
  answer names the original order. A refusal drops the record so a later cycle recomputes the
  residual. An entry leg is tracked at commitment; if its acknowledgement is lost it becomes a
  durable `EntryAttempt`, and `recover_entry_attempts` either adopts the identity a later
  observation carried or resubmits under the same reference, re-creating the `PendingOrder` with
  its original window start.
- **Settlement.** `open_position` drops the mandate's closure and cancellation entries along with
  its pending legs, so nothing keyed by a recyclable identifier outlives the mandate.

### 3. `src/types/snapshot.rs` — durable state

`Snapshot` gains `entry_attempts: Vec<EntryAttempt>`. `EntryAttempt` carries the strategy,
instrument, venue, client reference, price, size, expiry and `created_at`, the instant its window
opened. Everything else on the restart list was already durable.

## Why the obvious partial fixes do not pass

| Attempt | Fails on |
|---|---|
| Folding cumulative quantities in as increments, or adding the list on top of the feed | v6, v7, v13, v25, v25b |
| A closed window end | v19 |
| Resolving observations by venue identifier only | v9, v13 |
| Equal split, dropped remainder, remainder to the largest order, ordering by identifier | v20–v25b, v28 |
| Basis of submitted size rather than outstanding | v24b, v28 |
| Leaving a zero-share covered order working | v28 |
| Neutralizing the full leg, ignoring attributed quantity, or ignoring working quantity | v6, v16, v20, v24, v25, v25b and others |
| Treating a lost acknowledgement as a refusal, or retrying under a fresh reference | v9, v13, v27 |
| Treating a refusal as a committed order | v14 |
| Not persisting direct progress, block references or committed neutralizations | v11, v17, v13 |
| Completion from custody inventory | v5a, v5b, v10, v11b and others |
| Attribution shared across mandates on one instrument | v10, v13 |
| Dropping an entry leg whose acknowledgement was lost | v27 |
| Leaving closure state behind when a mandate settles, or resolving the venue identifier first | v26 |
| Resetting the client reference counter on restart | v13 |
| Neutralizing an unconfirmed leg | v15 |
| A fill event stamped at the window end counted | v29 |
