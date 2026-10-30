# 07 — `OrderGateway` design

Replaces `PolyClient::place_order_with_token_id`, `PolyClient::cancel_order`, and the
`OrderSigner`. This is the port that makes lost acknowledgements and duplicate execution
testable.

## Trait

```rust
#[async_trait]
pub trait OrderGateway: Send + Sync {
    async fn submit(&self, request: OrderRequest) -> Result<Accepted, SubmitError>;
    async fn cancel(&self, order_id: &OrderId) -> Result<(), CancelError>;
    async fn open_orders(&self) -> Result<Vec<OpenOrder>, PortError>;
}

pub struct OrderRequest {
    pub client_ref: ClientRef,
    pub market_id: MarketId,
    pub token_id: TokenId,
    pub side: Side,
    pub qty: Qty,
    pub price: Price,
    pub kind: OrderKind,
}

pub enum OrderKind { RestingLimit, ImmediateTaker }

pub struct Accepted { pub order_id: OrderId, pub client_ref: ClientRef }

pub enum SubmitError {
    Rejected(String),
    AcknowledgementLost,
    Unavailable,
}
```

## `ClientRef` — the dedupe primitive

`ClientRef` is a caller-supplied string on every submission. The venue treats it as an
idempotency key:

- first submission with a given `client_ref` → new order, `Accepted`
- any later submission with the **same** `client_ref` → the venue returns the *original*
  `Accepted` and creates nothing new
- different `client_ref` → a genuinely new order, always

The engine chooses `client_ref` values. That choice is the whole of V9. If the engine
derives it from something stable about the unresolved exposure, a retry after a lost
acknowledgement is absorbed. If it generates a fresh value per attempt, the retry executes
a second economically identical order and V9 fails.

The task must not tell the agent this. The port documentation states only that
`client_ref` is a caller-supplied reference the venue deduplicates on. Deriving a stable
one is the agent's problem.

## `AcknowledgementLost`

The critical error. Semantics:

> The venue **accepted and may have executed** the order. The response did not arrive.
> The `order_id` is unknown to the caller.

This is not a failure to submit. The harness, on a schedule-marked lost-ack cycle:

1. creates the order internally, exactly as on success
2. records it under its `client_ref`
3. returns `Err(SubmitError::AcknowledgementLost)`

The order is live. It may fill on a later cycle. A solution that treats this as "did not
happen" and blindly resubmits with a fresh `client_ref` doubles the position — which is
precisely partial fix P4.

`open_orders()` exists so a correct solution *can* reconcile rather than guess. Using it is
optional; the invariant, not the method, is graded.

## Cancellation

`cancel` fails with `CancelError::NotFound` when the order already filled or was already
cancelled. The source repository logs this and proceeds
(`filled_order_checker.rs:256-262`), which is one of the reasons a resting order can be
believed dead while still live. The harness reproduces the failure faithfully.

## Recorded history — the primary verifier surface

The harness records an append-only log:

```
(cycle, client_ref, market, token, side, qty, price, kind, outcome)
```

Verifier assertions read this log:

- **hedge count** — number of `ImmediateTaker` submissions that were *economically
  distinct*, i.e. counted by distinct `client_ref` **and** by orders actually created at
  the venue. A retry under the same `client_ref` counts once. A resubmission under a fresh
  `client_ref` counts twice, which is the failure.
- **hedge quantity** — asserted against unresolved exposure, not intended leg size (V6).
- **cancellations** — asserted in V5 as one of two permitted resolutions.
- **net exposure** — derived from created orders plus fills (V8).

None of this inspects source code. It observes what the engine did to the venue.

## Determinism

No wall clock, no randomness, no I/O. Failure injection, lost acknowledgements, and
rejection are schedule-driven and reproducible. Per-submission latency is expressed in
whole cycles, so a solution that serialises all markets behind one lock across
submissions measurably misses per-market deadlines (P9).
