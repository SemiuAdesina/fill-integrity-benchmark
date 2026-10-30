# 01 — Sanitized source-file allowlist

Explicit allowlist. Nothing is admitted by "copy the repo then delete". Every entry below
was opened and read in the source repository before admission.

Source repository (read-only): `/Users/macbok/Documents/poly-arb-engine`
Source tree size: 162 `.rs` files. **Admitted: 8. Admission rate: 4.9%.**

## A. Admitted from source (adapted, not verbatim)

| # | Source path | Src LoC | Role in task | Adaptation |
|---|---|---|---|---|
| A1 | `src/types/pending_order.rs` | 41 | Tracked-order record | Extend with attribution fields; drop `market_question` |
| A2 | `src/types/position.rs` | 70 | Position record | Keep constructor arithmetic; replace `Utc::now()` with injected clock |
| A3 | `src/engine/pending_orders.rs` | 84 | Tracked-order collection | Keep public method set; remove `println!` |
| A4 | `src/engine/positions.rs` | 53 | Position collection | Keep public method set; remove `println!` |
| A5 | `src/app/filled_order_checker.rs` | 326 | **Defect site**: fill classification + orphan hedge | Threshold pre-corrected; live clients replaced by injected ports |
| A6 | `src/engine/fishing.rs` | 133 | Leg sizing | Admit sizing functions only; drop price/threshold helpers |
| A7 | `src/app/order_cleanup.rs` | 29 | Cancellation of near-expiry tracked orders | Invoked directly by the driver in correct order |
| A8 | `src/config/settings.rs` | 108 | Engine parameters | Reduced from 33 fields to 6; no env/dotenv, no secrets |

## B. New files authored for the task (no source ancestry)

| # | Path (indicative) | Role |
|---|---|---|
| B1 | `ports/balance_source.rs` | `BalanceSource` trait — deterministic ERC-1155 balance reads |
| B2 | `ports/order_gateway.rs` | `OrderGateway` trait — submit / cancel / query, with lost-ack and dedupe |
| B3 | `ports/fill_event_source.rs` | `FillEventSource` trait — per-order venue fill events |
| B4 | `ports/state_store.rs` | `StateStore` trait — restart snapshot persistence |
| B5 | `ports/clock.rs` | `Clock` trait — virtual time |
| B6 | `sim/schedule.rs` | Declarative event-schedule representation |
| B7 | `sim/harness.rs` | In-memory port implementations driven by a schedule |
| B8 | `engine/cycle.rs` | Deterministic cycle driver replacing `trading_loop.rs` |

## C. Deliberately NOT admitted although topically adjacent

| Source path | Why excluded |
|---|---|
| `src/app/trading_loop.rs` | Contains the WebSocket spawn, drawdown gate, FlareSolverr health probe, and `tokio::select!` sleep loop. Replaced wholesale by B8. Copying it would import three unrelated defects. |
| `src/app/position_redemption.rs` | Redemption/merge is downstream of attribution. No V-case needs it. Admitting it would widen scope and violate CURVD *Reduced*. |
| `src/engine/drawdown.rs` | Independent confirmed defect (equity blindness). Out of scope. |
| `src/clients/chain/token_balance.rs` | Live Polygon RPC. Replaced by B1. |
| `src/clients/poly_api/**` | Live CLOB, HMAC auth, FlareSolverr. Replaced by B2/B3. |
| `src/execution/signer/**` | EIP-712 signing with real keys. No behavioural role in attribution. |
| `src/app/order_placement/order_execution/mod.rs` | Its *effect* (asymmetric leg sizes) is reproduced via A6 inside the schedule fixtures. The module itself pulls in the client stack. |

## D. Scope decisions recorded

**D1 — The cancellation-ordering defect is excluded from scope.** In the source,
`trading_loop.rs:82` calls `cleanup_expired()` (retains `!should_cancel()`) immediately
before `:83` calls `cancel_orders_near_expiry()` (selects `should_cancel()`), so the
cancel set is always empty. This is a real, separately confirmed bug. The task driver
(B8) calls cancellation **before** cleanup, so the defect is absent from the starting
state and cannot be accidentally graded. Rationale: CURVD *Reduced* — one problem.

**D2 — The per-leg threshold defect is pre-corrected.** See `15-starting-broken-state-design.md`.

**D3 — `market_question` is dropped everywhere.** It carries production market text.
Markets are identified by opaque `MarketId` in the task.
