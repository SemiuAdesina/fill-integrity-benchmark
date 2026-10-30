
# Repair fill attribution in the arbitrage engine

## Where to work

The Rust project you must modify is at /app.

The final deliverable is the corrected working Rust source tree under /app. Leave it in a state that builds and runs.

You do not need to write a report, a summary file, or any notes. Only the corrected source tree is graded.

All paths in this task are absolute.

## The objective

This task has one focused objective. Repair attributable fill processing, and keep unrelated behaviour working.

## Problem

This application places two related limit orders for each market:

1. A YES order
2. A NO order

When both orders fill, the application creates a completed position.

When only one order fills, the application may place a hedge order for the missing exposure.

The current fill processing is incorrect. It uses the wallet's total token balance to decide whether a specific tracked order has filled.

A wallet balance is shared state. It may include tokens from:

1. An earlier trading round
2. A previous application run
3. A different tracked market
4. A hedge order
5. An order that is no longer being tracked
6. A delayed balance update

Because of this, the application can treat old tokens as a new fill. It can also place the same hedge more than once, or create a position from tokens that do not belong to the current pair of orders.

## Your task

Repair the fill processing logic.

The application must only create a position from fills that belong to the currently tracked YES and NO orders.

You may change the internal design, data structures, and implementation details. Nothing depends on a specific helper name, struct name, field name, or algorithm.

Keep unrelated behaviour unchanged. The files under /app/spec describe the public contract you must keep and the venue rules you can rely on.

## Required behaviour

### 1. Genuine paired fills

When the tracked YES and NO orders both fill:

1. Create exactly one position.
2. Use the correct filled quantity for each leg.
3. Do not place a hedge order.
4. Do not create the same position again during later processing cycles.

### 2. Genuine one sided fill

When only one tracked leg fills:

1. Keep the unfilled leg correctly tracked.
2. Hedge only the unresolved exposure when a hedge is allowed.
3. Submit the required hedge no more than once for the same unresolved exposure.
4. Do not hedge the full original quantity when part of the missing leg has already filled.

### 3. Historical wallet balances

A token balance that existed before an order began being tracked must not count as a fill for that order.

For example, if the wallet already contains 40 YES tokens before a new YES order is placed, those 40 tokens must not prove that the new YES order filled.

### 4. Hedge attribution

Tokens received from a hedge order must not be counted as fills from the original resting order.

The corrective action must not change the fill signal in a way that causes another unnecessary corrective order.

### 5. Repeated and stale observations

The balance source may return the same old value for several processing cycles.

Repeated or stale observations must not:

1. Create duplicate positions.
2. Submit duplicate hedge orders.
3. Remove live orders from tracking.
4. Increase exposure beyond the intended quantity.

The system must still make progress after newer information becomes available.

### 6. Fill during hedge submission

A resting order may fill while a hedge order is being submitted.

The final exposure must not exceed the amount needed to complete the pair.

The application must reconcile the new fill before it submits another economic order.

### 7. Lost acknowledgement

The venue may accept a hedge order, but the response may be lost.

A retry must not submit the same economic hedge twice.

The application must be able to reconcile an accepted order after an acknowledgement failure.

### 8. Shared token identifiers

Two tracked markets may refer to the same token identifier.

A balance change for one market must not automatically count as a fill for another market.

Fill state must remain separate for each tracked order and market.

### 9. Restart recovery

The application may restart while orders are still unresolved.

After restoring its saved state:

1. Historical balances must not become new fills.
2. Previously acknowledged progress must not be counted again.
3. Live unresolved orders must remain recoverable.
4. Completed exposure must not be hedged again.

### 10. Later legitimate hedging

A market that has already been hedged once may later need another genuine hedge. For example, an earlier attempt may be refused by the venue, so nothing was created and the exposure is still open.

A market must stay able to make progress. Do not block all future hedging for a market after a single attempt.

### 11. Independent markets

Markets are independent.

A delay, a refusal, or a completed hedge in one market must not stop another market from being tracked, filled, hedged, or completed.

### 12. Order tracking

A live resting order must not silently disappear from tracking.

It may be removed only when it is confirmed as:

1. Filled
2. Cancelled
3. Expired under the existing rules
4. Replaced by another correctly tracked order
5. Otherwise fully resolved

## Important constraints

1. The task must remain fully offline.
2. Do not add live network calls.
3. Do not require a wallet, private key, RPC endpoint, exchange account, or real funds.
4. Do not use wall clock sleeps to make the logic work.
5. Do not solve the problem by disabling all hedging.
6. Do not clear all pending state when information is uncertain.
7. Do not use one permanent flag that prevents every future hedge for a market.
8. Do not hardcode test identifiers or event schedules.
9. Do not change behaviour that is unrelated to fill processing.
10. Do not rely on the wallet's absolute balance alone as proof that a tracked order filled.

## Validation

Grading runs a behavioural suite outside this environment against the frozen public contract described in /app/spec. That suite is not present in /app, so you cannot run it yourself.

To check your work locally, build the crate and use the offline reproducer:

```
cargo build --offline
cargo run --offline --bin reproduce
```

The reproducer at /app/src/bin/reproduce.rs drives the engine through two of the failure modes below using in-memory fakes: a historical wallet balance completing a leg that never filled, and two markets sharing a token identifier completing each other's legs. Against the unmodified source it reports both defects. It is a diagnostic aid, not the grader, and passing it is necessary but not sufficient. You are free to extend it or add your own tests.

Your implementation should work for:

1. Symmetric paired fills
2. Asymmetric paired fills
3. Genuine one sided fills
4. Partial quantity fills
5. Repeated stale observations
6. Historical token balances
7. A fill arriving during hedge submission
8. A lost order acknowledgement
9. Shared token identifiers
10. Restarted processing
11. Later legitimate hedge activity
12. Independent progress across markets
13. Different deterministic event orderings

The final implementation must preserve both safety and progress.

Safety means the engine does not create false positions, duplicate orders, or excess exposure.

Progress means genuine fills are still detected, genuine positions are still created, and valid one sided fills can still be hedged.

You have 7200 seconds to complete this task. Do not cheat by using online solutions or hints specific to this task.
