# Reconciliation contract

`venue-semantics.md` states what the venue and the feed guarantee. This document
states what the engine must do with those observations. Both are normative, and
where either disagrees with the code as it stands, the code is what changes.

Nothing here prescribes an internal design. Every rule below is a statement
about observable outcomes: which pairings are recorded, at what size, which
corrective orders are sent, at what size, and in what order relative to the
cancellations that precede them.

## Attribution progress per order

The engine holds one attribution record per order it submitted. That record is
identified by the `client_ref` the engine chose, because it is the only key the
engine always knows: the venue's `order_id` is unknown until the venue names it,
and an order whose acknowledgement was lost has none. Once the venue does name
an `order_id` for a `client_ref`, either key resolves to the same record.

An order's attributed quantity has two independent parts, and they accumulate
differently.

**Direct progress is cumulative.** Two observations report it: the
`cumulative_filled_qty` carried on a fill event, and the `filled_qty` carried on
an entry of the open-order list. Both state the same thing — the total quantity
that order has filled so far — and neither is an increment. Direct progress is
therefore the **maximum** ever observed for that order, never a running sum.
Folding either figure in as an increment inflates the order when the feed
redelivers, and adding one source on top of the other double counts the same
base units, because the two describe one quantity rather than two.

**Block progress is additive.** A share derived from a block fill is an
increment, and is added to the record. Redelivery is prevented here not by a
maximum but by `block_ref` idempotency: the same `block_ref` is the same match
however many times the feed delivers it, and it contributes exactly once.

An order's attributed quantity is its direct progress plus its block progress.
Its outstanding quantity is its size minus its attributed quantity, floored at
zero.

## Block finality

A block closes every order it names. Once the engine has observed a block,
**every tracked order named in its `covers` list is finalized**, whether or not
the block allocated that order anything. This includes:

- an order that was eligible and received a share;
- an order that was eligible and received zero because the truncated division
  gave it nothing;
- a tracked covered order that was **not eligible at all**, because the block's
  match timestamp fell outside that order's attribution window, or because the
  order already had no outstanding quantity.

Finalization is a statement about the venue rather than about the arithmetic:
the venue has closed the order and cancelled whatever part of it the block did
not fill. A finalized order will never acquire another base unit, is not live,
and is not still working. It therefore contributes no outstanding quantity to a
later block's basis, and no still-working quantity to any residual exposure.

A block whose eligible set is empty allocates nothing, and still finalizes every
tracked order it covers.

## Residual exposure

The unresolved exposure on a leg is

```
residual = required quantity
         - quantity already attributed to that leg
         - quantity still working in committed neutralizations for that leg
```

The required quantity is the leg's size.

The second term counts quantity attributed to the leg's own resting order **and**
quantity attributed to neutralization orders the engine has already placed for
the same mandate and venue. A neutralization fill resolves the exposure; it must
not also be counted a second time as a fill of the original resting order.

The third term is what stops consecutive cycles from sending the same exposure
twice. A neutralization the engine has committed to counts as working until it
resolves:

- one the venue has acknowledged contributes its own outstanding quantity;
- one whose acknowledgement was lost contributes its **full size**, because the
  venue may have created it and the engine cannot yet see it;
- one that has been finalized contributes **nothing**, whatever it managed to
  fill, because the venue has closed it and the gap it leaves behind is real.

Neutralize only a strictly positive residual. Quantities are whole base units,
so a residual of one unit is a real residual, and a residual of zero or less is
not an exposure.

## Order visibility and the closure sequence

The open-order list is point-in-time truth about what is live. An order absent
from it has not necessarily been withdrawn: it may have filled, with the fill
not yet through the feed. An order is therefore in one of three states — still
listed and resting; withdrawn, because the engine itself cancelled it; or
unconfirmed, meaning neither of those. **An unconfirmed leg is never neutralized
against.** Neutralizing quantity the desk already owns doubles the exposure the
correction was meant to close.

Correcting a resting leg is not instantaneous. A resting order may not be
treated as gone merely because a correction has become necessary. The engine
first moves that order into closure, cancelling or replacing it at the venue,
and only once that cancellation boundary has passed may it issue the corrective
neutralization for whatever residual remains. The ordering exists so that a fill
already in flight has a cycle in which to land and be attributed before another
economic order is sent. What is required is the sequence of observable effects —
closure of the resting order first, corrective order afterwards — not any
particular internal mechanism for achieving it.

## Submission outcomes

- **Accepted.** The order exists and the venue has named it.
- **AcknowledgementLost.** The venue may or may not have created the order, and
  its identifier is unknown. The `client_ref` is retained, the order is treated
  as potentially working, and any retry uses the **same** `client_ref`, so the
  venue's idempotency resolves it onto the original order. A retry under a
  freshly minted reference places the same economic exposure a second time.
  Identity is recovered when a later observation carries that `client_ref`.
- **Rejected** or **Transport.** No economic order was created by that attempt.
  The exposure is still open, and a later cycle may recompute the residual and
  try again. A mandate must not be latched shut by one failed attempt.

## What must survive a restart

A restart drops all in-memory state. The venue and the custody account keep
theirs. Whatever the engine needs in order not to redo settled work, or to avoid
reinterpreting old evidence as new, has to be persisted and restored. That
includes at least:

- each tracked order's **original** attribution window start — a restart that
  reopens a window admits an observation from outside the original one;
- the fill-feed cursor;
- attribution progress already accumulated, and each order's finalized state;
- the set of `block_ref` values already absorbed — a forgotten one counts a
  block twice;
- orders still being tracked, and work still in progress, including closure
  state;
- committed neutralizations, including unacknowledged ones, and the `client_ref`
  identity a retry depends on;
- the client-reference sequence — a reset counter regenerates a reference the
  venue already knows, and the venue deduplicates it onto an unrelated order.

Pre-existing custody inventory must not be reinterpreted as new fills after a
restart, quantity already paired must not be neutralized again, and live
unresolved orders must remain recoverable.

## Determinism

Given the same observations in the same order, the engine reaches the same
observable outcome. The iteration order of whatever container holds the engine's
orders must never decide an attribution outcome, and no quantity decision may
depend on floating point or on a comparison tolerance.
