# 06 — `BalanceSource` design

Replaces `src/clients/chain/token_balance.rs` (live ERC-1155 `balanceOf` over Polygon RPC).

## Trait

```rust
#[async_trait]
pub trait BalanceSource: Send + Sync {
    async fn balance_of(&self, token: &TokenId) -> Result<Qty, PortError>;
    async fn balances_of(&self, tokens: &[TokenId]) -> Result<Vec<Qty>, PortError>;
}
```

`balances_of` exists because the source repository issues two independent reads per market
(`filled_order_checker.rs:36-37`) and can therefore observe a torn pair — YES read before a
fill, NO read after. Keeping both methods lets the harness exercise that skew.

## Semantics the harness guarantees

1. **Absolute, shared, aggregate.** Returns the wallet's *total* holding of `token`. It
   carries no information about which order produced it. Two markets sharing a token id
   see the same number.
2. **Includes everything.** Historical inventory, fills of tracked resting orders, hedge
   fills, and inflows the engine never caused are indistinguishable in the return value.
3. **Monotonic only where the venue is.** Merges and redemptions decrease it. The engine
   must not treat a decrease as a negative fill.
4. **Lag is explicit.** The harness can hold a stale value for a configured number of
   cycles (V7). Reads during the lag window return the *old* value with no error and no
   marker.
5. **Failure is a first-class outcome.** `PortError::Unavailable` is returned on cycles the
   schedule marks as failed, mirroring the source repository's `continue`-on-error path.

## Determinism

No clock, no randomness, no I/O. The value returned on cycle *n* is a pure function of the
schedule and the venue state produced by prior cycles. Repeated calls within one cycle
return the same value.

## Why the defect survives a correct-looking use of this port

The port is honest — it never claims to attribute. The shipped engine calls `balance_of`
and treats the result as "shares this order received". Nothing in the port's contract
supports that reading. The instruction says so plainly, without saying what to do instead.

## What the harness records

Every call is logged as `(cycle, token, returned_qty)`. The verifier uses this only for
diagnostics on failure, never as a pass criterion — asserting on read *patterns* would be
source inspection by another route.

## Anti-shortcut property

A solution that polls this port repeatedly inside one cycle hoping the value "settles"
gains nothing: the value is fixed for the cycle. Combined with the deadline in doc 12
(I13), sleep-or-retry-until-stable approaches fail rather than merely being slow.
