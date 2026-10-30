# 02 — Per-file inclusion justification

One entry per admitted file. Each states what the file contributes, what would break
without it, and what is stripped on the way in.

---

## A1 — `src/types/pending_order.rs` (41 LoC)

**Contributes.** The tracked-order record. Source fields: `condition_id`,
`market_question`, `token_id`, `side`, `order_id`, `limit_price`, `size`, `expiry_time`,
`created_at`. Plus `should_cancel()` — the 300-seconds-before-expiry rule.

**Why required.** This is the record the whole task is about. The accepted proposal
requires each tracked order to carry market, token, side, intended quantity,
acknowledged filled quantity, attribution checkpoint, resting identity, hedge identity,
accepted-but-unacknowledged state, resolved exposure, and persisted restart state. Six of
those are new fields hung on this struct; the other five are relations keyed by it.

**Stripped.** `market_question` (production market text). `Utc::now()` in the
constructor → injected `Clock`. `String` side → `Side` enum, so `side == "YES"`
stringly-typed comparisons cannot silently mismatch.

**Kept unchanged.** `should_cancel()` semantics and the 300s constant — frozen public
behaviour, asserted by an unrelated-behaviour regression case.

---

## A2 — `src/types/position.rs` (70 LoC)

**Contributes.** The position record and its derived arithmetic:
`entry_cost = entry_yes_price + entry_no_price`,
`total_usd_cost = entry_yes_price * size_yes + entry_no_price * size_no`.
Also `should_exit_now()`, `is_too_close_to_expiry()`, `is_expired()`,
`seconds_until_expiry()`.

**Why required.** "Exactly one position with correct sizes" is the primary observable in
V1, V2, V3, V5, V8. The verifier reads `size_yes`, `size_no`, `entry_*_price`, and
`total_usd_cost`. Without this record there is nothing to assert on.

**Stripped.** `market_question`; `Utc::now()` → injected `Clock`.

**Kept unchanged.** All four time predicates and both derived-cost formulas — frozen.

---

## A3 — `src/engine/pending_orders.rs` (84 LoC)

**Contributes.** `PendingOrderManager` over `Arc<RwLock<Vec<PendingOrder>>>`, with
`add_order`, `get_orders`, `get_orders_for_market`, `has_complete_set`, `remove_order`,
`get_orders_to_cancel`, `cleanup_expired`, `count`, `get_all_markets`.

**Why required.** Invariant I11 — a live resting order must not silently disappear from
tracking — is asserted directly against this collection. V5 checks that the still-live
order is either present here or was explicitly cancelled through the gateway.

**Stripped.** All `println!`. The `&order.order_id[..16]` slice in `add_order` (panics on
short ids, and the task uses short synthetic ids).

**Kept unchanged.** The nine public method signatures and their semantics. An agent may
add methods; changing the meaning of these fails the frozen-surface cases.

---

## A4 — `src/engine/positions.rs` (53 LoC)

**Contributes.** `PositionManager` with `add_position`, `get_positions`,
`get_positions_to_exit`, `remove_position`, `cleanup_expired`, `count`.

**Why required.** "Exactly one position" is counted here. `get_positions()` is the
verifier's read path for V1–V3, V5, V8, V10, V11.

**Stripped.** `println!` in `add_position` and `remove_position`.

**Kept unchanged.** Six public method signatures and semantics.

---

## A5 — `src/app/filled_order_checker.rs` (326 LoC) — **defect site**

**Contributes.** The function under repair. Three behaviours the task depends on:

1. **Pairing** (`:19-27`) — group tracked orders by `condition_id` into
   `(Option<&PendingOrder>, Option<&PendingOrder>)`. Note the source keeps only the
   *last* YES and *last* NO per market; with duplicates, earlier orders are silently
   dropped. Retained as-is: it is a genuine part of the surface the agent must reason
   about when two markets share a token.
2. **Classification** (`:52-54`) — the defect. Absolute wallet balances compared against
   a size threshold, with no notion of which order those tokens came from.
3. **Orphan hedge** (`:227-298`) — `attempt_orphan_hedge`, the compensating action. It
   cancels the stale leg (`:256`, failure only logged), places a taker BUY for the full
   `unfilled.size` (`:268-278`), then swaps the tracked entry for one carrying the *same*
   size (`:283-293`). This is what makes the defect self-amplifying: the hedge increases
   the very balance the faulty detector reads.

**Why required.** Removing any of the three removes the task. (2) is the bug, (3) is the
feedback loop that makes it hard, (1) is where shared-token ambiguity surfaces.

**Stripped.** `PolyClient`, `OrderSigner`, `AlertEvent`/Telegram, all `println!`/
`eprintln!`, all `&id[..16]` slices, `check_orderbook_hints` (live orderbook, no
behavioural role).

**Pre-corrected before shipping.** `min_required` becomes per-leg. See doc 15.

**Left broken.** Fill inference from unattributed absolute balance.

---

## A6 — `src/engine/fishing.rs` (133 LoC)

**Contributes.** `calculate_minimum_order_size(price, min_usd) = (min_usd/price).ceil().max(1.0)`
and `calculate_kelly_order_size(...)`.

**Why required.** Asymmetric leg sizes must be *derivable from the engine's own sizing
rule*, not asserted by fiat in a fixture. The measured values
`calculate_minimum_order_size(0.10, 5.0) = 50` and
`calculate_minimum_order_size(0.87, 5.0) = 6` are what make V2's asymmetric pair
realistic rather than contrived. An agent inspecting the fixtures must be able to see the
sizes are legitimate engine output.

**Stripped.** `calculate_fishing_prices`, `should_place_fishing_orders`,
`should_redeem_filled_set`, `floor_to_tick` — pricing and redemption, not sizing.

**Kept unchanged.** Both sizing functions, verbatim arithmetic.

---

## A7 — `src/app/order_cleanup.rs` (29 LoC)

**Contributes.** Iterate `get_orders_to_cancel()`, cancel each through the venue, remove
from tracking on success.

**Why required.** V5 permits two correct outcomes for the still-live order: remain
tracked, **or** be explicitly cancelled. The second is only expressible if a cancellation
path exists. It is also the honest resolution for an order the engine no longer wants.

**Stripped.** `PolyClient` + `OrderSigner` → `OrderGateway`. `println!`/`eprintln!`.

**Reordered by the driver.** Cancellation runs *before* `cleanup_expired()`, not after
(scope decision D1).

---

## A8 — `src/config/settings.rs` (108 LoC)

**Contributes.** Engine parameters.

**Why required.** `min_order_value` feeds A6's sizing; hedge gating needs its combined-cost
cap.

**Reduced from 33 fields to 6:** `min_order_value`, `orphan_hedge_max_combined`,
`cancel_lead_seconds`, `max_pending_orders`, `fill_tolerance`, `hedge_retry_limit`.

**Removed entirely.** `dotenv`, every RPC URL, `private_key`, `proxy_address`,
`api_key`/`api_secret`/`passphrase`, Telegram token and chat id, FlareSolverr URL, every
wallet and network field. Constructed literally in code; no environment read at any point.
