# 03 — Exclusion list

Everything in the source repository that must never reach the package. Grouped by reason.
Sizes measured 2026-08-05.

## Class 1 — Secrets and credentials (never opened, never read, never copied)

| Path | Contents | Handling |
|---|---|---|
| `.env` | Live private key, RPC URL with embedded credentials, CLOB API key/secret/passphrase, Telegram bot token | **Never opened during this work.** Not read, not printed, not hashed, not copied. |
| `verify_creds.py` | Credential verification helper | Excluded. |
| `.env.example` | Placeholder keys | Excluded. Task reads no environment at all, so even a placeholder template is unnecessary and would invite an agent to look for env config. |
| `poly-bot.service` | systemd unit; contains deployment user, absolute host paths | Excluded. |

`.env` is the single highest-risk file in the repository. It is not in the allowlist, is
not referenced by any admitted file after adaptation, and A8 removes the `dotenv` call
that would read it.

## Class 2 — Unrelated subsystems

| Path | Files | Why excluded |
|---|---|---|
| `src/hermes/**` | — | Separate strategy. No relation to fill attribution. |
| `src/btc_alpha/**` | 31 | Separate BTC strategy with its own executor, signer, Hyperliquid WebSocket, and state machine. |
| `src/telegram/**` | — | Alerting. `AlertEvent` sends are dropped during adaptation. |
| `src/clients/poly_api/flaresolverr/**` | 7 | Cloudflare bypass. |
| `src/clients/poly_api/**` (rest) | ~15 | Live CLOB REST, HMAC auth, order builder, WebSocket. Replaced by `OrderGateway` / `FillEventSource`. |
| `src/clients/chain/**` | 4 | Live Polygon RPC, ERC-1155 reads, `mergePositions`. Replaced by `BalanceSource`. |
| `src/execution/signer/**` | — | EIP-712 signing with real keys. |
| `src/bin/fetch_hn*` | 5 | Unrelated binary. |
| `src/engine/drawdown.rs` | 1 | Independent confirmed defect (deployed capital counted as loss). Out of scope. |
| `src/engine/discovery/**`, `strategy.rs`, `risk.rs` | 4 | Market discovery and risk gating. Not on the attribution path. |
| `src/app/startup/**`, `initialization.rs` | 4 | Client construction from secrets. |
| `src/app/order_placement/fishing_orders/**` | 3 | Includes the random `min_profit_pct` selection — nondeterminism. |
| `src/app/ws_fill_listener.rs` | 1 | WebSocket plumbing. Replaced by `FillEventSource`. |
| `src/app/position_redemption.rs` | 1 | Downstream of attribution; excluded for CURVD *Reduced*. |

## Class 3 — Production data and logs

| Path | Size | Why excluded |
|---|---|---|
| `logs/` | 1.0 MB | Production logs. May contain order ids, transaction hashes, wallet addresses. |
| `data/` | 516 KB | Captured market data. |
| `analysis/` | 16 KB | Analysis outputs. |
| `hermes_state.json` | 2.7 MB | Live strategy state. |
| `btc_alpha_state.json` | 912 KB | Live strategy state. |
| `hermes_alignments.jsonl` | — | Live strategy output. |
| `hermes.log` | 84 KB | Production log. |

## Class 4 — Generated and VCS artefacts

| Path | Size | Why excluded |
|---|---|---|
| `target/` | 5.7 GB | Build output. |
| `.git/` | — | Full history, including the pre-existing dirty worktree. |
| `__pycache__/` | 8 KB | Python bytecode. |
| `Cargo.lock` (source) | — | Locks the full 6-binary dependency graph. The task authors its own lock over a much smaller set (doc 17). |
| `.DS_Store`, `._*`, `__MACOSX` | — | macOS metadata. Scrubbed before archiving. |

## Class 5 — Identifying strings scrubbed from admitted content

Even inside allowlisted files, these must not survive:

- the absolute path `/Users/macbok/Documents/poly-arb-engine` and any `/Users/macbok/**` path
- the repository name `poly-arb-engine`
- the venue names Polymarket / Polygon / FlareSolverr / Telegram, and `0x`-prefixed
  contract addresses, selectors such as `0x00fdd58e`, and the CLOB WebSocket URL
- production `condition_id` and `token_id` values (66-char and 77-digit); replaced by
  synthetic `MarketId("m1")` / `TokenId("t1")`
- production order ids and transaction hashes
- the author's name, email, and any account identifier inside package files

Domain vocabulary is generalised: "conditional token" rather than ERC-1155, "venue"
rather than the exchange name. The task remains a realistic two-leg market-making engine
without naming the operator or the venue.

## Method

Allowlist-only construction. Files are copied **into** an empty tree one at a time, each
opened and read first (doc 02). At no point is the repository copied wholesale and pruned.
