# 19 — Security and redaction plan

The task is derived from a live trading repository holding real credentials, real market
identifiers, and real production logs. Nothing of that may reach the package.

## Principle

The package is **written fresh**, not copied and cleaned. Every admitted file (doc 01) is
re-authored against the source's *behaviour*, not pasted from it. Redaction is therefore a
verification step confirming a property that construction already guarantees — not the
mechanism relied on to achieve it.

## Never read, never copied

Per the user's explicit list, and confirmed by the exclusion list (doc 03):

- `.env` — **never opened at any point in this work**, not read, not copied, not hashed
- private keys, seed phrases, API secrets, wallet credentials
- production authentication tokens
- personal data
- production logs containing identifiers

Additionally excluded as credential-adjacent: `.env.example`, `verify_creds.py`,
`poly-bot.service`, and every module that consumes credentials (signer, chain clients, API
clients, Telegram).

## Class 5 — identifying strings to verify absent

The construction approach means these should not appear. The scan confirms it.

| # | Pattern | Why |
|---|---|---|
| 1 | `/Users/macbok/...` and the string `macbok` | local absolute paths, username |
| 2 | `poly-arb-engine` | source repository name |
| 3 | Venue and vendor names (the trading venue, FlareSolverr, Telegram, the RPC provider) | attribution and de-anonymisation |
| 4 | `0x` followed by 40 or 64 hex characters | addresses, private keys, tx hashes |
| 5 | The ERC-1155 `balanceOf` selector `0x00fdd58e` | chain-specific fingerprint |
| 6 | Any `wss://` or `https://` URL | endpoints; the package needs none |
| 7 | Production condition ids, token ids, order ids | real market data |
| 8 | The author's name and email | from git config and file headers |
| 9 | `PRIVATE_KEY`, `API_KEY`, `SECRET`, `PASSPHRASE`, `MNEMONIC`, `SEED` | credential variable names |
| 10 | Long base64-looking runs (≥ 32 chars) | encoded secrets |

## Synthetic identifiers

The package uses obviously-fake ids throughout: `m1`, `m2`, `tA`, `tB`, `tC`, `ord-1`,
`hedge-1`. Short, readable, and unmistakably synthetic — no risk of a real id hiding among
plausible-looking hex.

Prices and sizes come from the real sizer's arithmetic but are ordinary round numbers
(0.10 / 0.87, 0.45 / 0.52) that describe no real market.

## Scan procedure

Run against the built package directory **and** against the extracted ZIP, so that anything
introduced during packaging is also caught:

1. `grep -rIn` each Class 5 pattern across every file
2. binary-file sweep — no `.png`, `.jpg`, `.pdf`, `.so`, `.rlib`, no `target/`, no `.git/`
3. dotfile enumeration — every hidden file listed and justified individually
4. `git` artefact check — no `.git`, no `.gitignore` carrying paths, no `.DS_Store`
5. entropy check — flag any token ≥ 32 chars matching `[A-Za-z0-9+/=_-]{32,}` and review
   each hit by hand
6. vendored-dependency exemption — vendor directories are scanned for the local-path and
   name patterns but exempted from the entropy check, which they would flood with false
   positives from checksums

Any hit blocks packaging. The scan is re-run after the final ZIP is produced, on the
extracted contents, and the result recorded.

## Canary handling

Restated from the user's constraints, and binding on this workspace:

- The canary is **private**. It is not printed in the final report, not written into the
  original repository, not committed to git, not included in screenshots, not placed in
  unrelated files, and not exposed in logs.
- **No canary value appears anywhere in this planning workspace.** Confirmed: these 20
  documents contain no canary string.
- Two destinations exist and both are required: inside every text file of the task, and the
  platform's separate `canary_string` upload field. The visible upload page does not show
  that field; its absence from view is not evidence it is unnecessary.
- It must **not** go into: Task name, Task category, Programming language, Subcategory, or
  the ZIP filename.
- The exact per-file comment syntax is **blocked** on the Official Contributor Guidelines.
  The older UUID-format syntax seen in an unrelated local task is not a valid template,
  because the accepted proposal supplies a different canary format. This is not inferred.

## Session-output hygiene

A redaction filter applied to shell output in this session targets 32-hex tokens. It does
**not** match the older UUID-format canary belonging to a different, already-submitted task;
that limitation was disclosed when it arose. The `attributable-fill-detection` canary has
not been printed, stored, or written to any file in this or the planning workspace.

## Original-repository safety

`/Users/macbok/Documents/poly-arb-engine` is read-only for the duration. Only
`git rev-parse`, `git status`, `git diff --stat`, `git stash list`, `grep`, `awk`, and file
reads have been used against it. Baselines are recorded at
`/tmp/polyarb_baseline_hashes.txt` and `/tmp/polyarb_baseline_status.txt` and
are re-verified at the end of every working session:

- branch `main`, HEAD `1f51817d2d556f7ece15ec4f0ecd189c732d0cdc`
- 63 `git status --porcelain` entries, 0 stashes
- `git diff --stat` 49 files changed, +1340 / −440
- aggregate SHA-256 over 162 `src/**/*.rs` files

## Licence and attribution

The package ships its own licence text as required by the guide. The source repository's
`LICENSE` (with its copyright holder name) is not copied. No file header names an author.

## Residual risk

The main residual risk is *conceptual* rather than textual: the task describes a real
trading strategy's failure mode. That is inherent to the task being useful, the strategy is
publicly documented in general terms, and no parameter, threshold, or identifier that would
make it actionable against a specific deployment is included. The six retained settings are
generic and carry default values chosen for the task, not the production values.
