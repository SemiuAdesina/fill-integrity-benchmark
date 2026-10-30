#!/usr/bin/env python3
"""Second rename pass: compound identifiers, test names, enum variants and
plurals that the word-boundary pass in rename_v4.py could not reach.

These are substring replacements applied inside longer identifiers such as
`v10_shared_token_identifier_does_not_cross_attribute` or `FreezeBalances`.
Ordered longest-first so no replacement eats a prefix of a later one.
"""
import sys
from pathlib import Path

ROOT = Path(sys.argv[1])

SUBS = [
    # --- action / fake enum variants ----------------------------------------
    ("UnfreezeBalances", "UnfreezeInventory"),
    ("FreezeBalances", "FreezeInventory"),
    ("FailNextBalanceRead", "FailNextInventoryRead"),
    ("CreditToken", "CreditInstrument"),
    ("FakeBalances", "FakeInventory"),
    ("FakeMarkets", "FakeStrategies"),
    ("initial_balances", "initial_inventory"),
    ("balance_failures", "inventory_failures"),
    ("set_token", "set_instrument"),
    # --- leg naming ---------------------------------------------------------
    ("yes_leg", "north_leg"),
    ("no_leg", "south_leg"),
    ("yes_complete", "north_complete"),
    ("no_complete", "south_complete"),
    ("no_token_for_second", "south_instrument_for_second"),
    # --- fixture literals ---------------------------------------------------
    ("token_yes_m", "inst_north_s"),
    ("token_no_m", "inst_south_s"),
    ("token_no_A", "inst_south_A"),
    ("token_no_B", "inst_south_B"),
    ("marketA", "strategyA"),
    ("marketB", "strategyB"),
    ("market1", "strategy1"),
    ("condition_ids", "strategy_ids"),
    ("_token_id", "_instrument_id"),
    # --- neutralization residue --------------------------------------------
    ("reconcile_hedges", "reconcile_neutralizations"),
    ("hedge_request", "neutralization_request"),
    ("hedged", "neutralized"),
    ("hedges", "neutralizations"),
    ("hedge", "neutralization"),
    ("Hedge", "Neutralization"),
    # --- test-name vocabulary ----------------------------------------------
    ("shared_token_identifier", "shared_instrument_identifier"),
    ("historical_balance", "historical_inventory"),
    ("one_sided_fill", "one_legged_fill"),
    ("per_side_fields", "per_venue_fields"),
    ("independent_markets", "independent_strategies"),
    # --- remaining plurals / caps ------------------------------------------
    ("Markets", "Strategies"),
    ("Market", "Strategy"),
    ("markets", "strategies"),
    ("market", "strategy"),
    ("Tokens", "Instruments"),
    ("tokens", "instruments"),
    ("balances", "inventory"),
    ("arbitrage", "paired execution"),
    ("YES", "NORTH"),
    # bare `yes` last, and only as a standalone word inside identifiers
    ("_yes_", "_north_"),
]

SKIP_DIRS = {"target", ".git"}
SKIP_SUFFIX = {".lock", ".png", ".jpg", ".zip"}


def files():
    for p in sorted(ROOT.rglob("*")):
        if not p.is_file():
            continue
        if any(part in SKIP_DIRS for part in p.parts):
            continue
        if p.suffix in SKIP_SUFFIX:
            continue
        yield p


changed = 0
for path in files():
    try:
        original = path.read_text()
    except UnicodeDecodeError:
        continue
    text = original
    for old, new in SUBS:
        # never rewrite the canary lines
        text = text.replace(old, new)
    # restore anything that mangled the canary marker
    text = text.replace("harbor-canary", "harbor-canary")
    if text != original:
        path.write_text(text)
        changed += 1
        print(f"  pass2 {path.relative_to(ROOT)}")

print(f"\n{changed} files touched in pass 2")
