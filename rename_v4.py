#!/usr/bin/env python3
"""Rewrite the task package from the prediction-market model into the synthetic
cross-venue paired-execution model.

Behaviour is preserved exactly: this is a pure identifier/vocabulary remap, so
the graded suite must still pass unchanged. Prose files are rewritten by hand
afterwards; this pass only makes the code and the literals consistent.

Ordering matters. Longer, more specific identifiers are replaced before the
short ones they contain, and enum paths are replaced before the bare enum name.
"""
import re
import sys
from pathlib import Path

ROOT = Path(sys.argv[1])

# (pattern, replacement) applied in order, each as a whole word.
PAIRS = [
    # --- enum paths first, before the bare `Side` -> `Venue` rename ----------
    (r"Side::Yes", "Venue::North"),
    (r"Side::No", "Venue::South"),
    # --- crate ---------------------------------------------------------------
    (r"arb_engine", "pairing_engine"),
    # --- composite field names, before their component words -----------------
    (r"entry_yes_price", "entry_north_price"),
    (r"entry_no_price", "entry_south_price"),
    (r"yes_token", "north_instrument"),
    (r"no_token", "south_instrument"),
    (r"yes_price", "north_price"),
    (r"no_price", "south_price"),
    (r"yes_depth", "north_depth"),
    (r"no_depth", "south_depth"),
    (r"yes_filled", "north_filled"),
    (r"no_filled", "south_filled"),
    (r"yes_order", "north_order"),
    (r"no_order", "south_order"),
    (r"yes_size", "north_size"),
    (r"no_size", "south_size"),
    (r"yes_balance", "north_observed"),
    (r"no_balance", "south_observed"),
    (r"size_yes", "size_north"),
    (r"size_no", "size_south"),
    (r"attempted_markets", "attempted_strategies"),
    # --- the inventory port (was a wallet balance) ---------------------------
    (r"BalanceSource", "InventorySource"),
    (r"token_balance", "instrument_inventory"),
    (r"cash_balance", "settlement_balance"),
    (r"balance_of", "inventory_of"),
    (r"balances", "inventory"),
    (r"balance", "inventory"),
    # --- neutralization (was hedging) ----------------------------------------
    (r"HedgePlan", "NeutralizationPlan"),
    (r"HedgeRecord", "NeutralizationRecord"),
    (r"plan_hedge", "plan_neutralization"),
    (r"hedge_limit_price", "neutralization_limit_price"),
    (r"hedge_slippage", "neutralization_slippage"),
    (r"hedge_missing_leg", "neutralize_missing_leg"),
    (r"hedged_markets", "neutralized_strategies"),
    (r"hedges", "neutralizations"),
    (r"hedge", "neutralization"),
    (r"hedging", "neutralization"),
    # --- identifiers ---------------------------------------------------------
    (r"MarketCandidate", "StrategyCandidate"),
    (r"MarketSource", "StrategySource"),
    (r"MarketId", "StrategyId"),
    (r"TokenId", "InstrumentId"),
    (r"condition_id", "strategy_id"),
    (r"token_id", "instrument_id"),
    (r"token_for", "instrument_for"),
    (r"market_ids", "strategy_ids"),
    (r"takers_on_token", "takers_on_instrument"),
    (r"taker_qty_on_token", "taker_qty_on_instrument"),
    (r"enter_new_markets", "enter_new_strategies"),
    (r"reconcile_open_markets", "reconcile_open_strategies"),
    (r"attempted_market", "attempted_strategy"),
    (r"best_ask", "best_offer"),
    (r"max_entry_cost", "max_pair_cost"),
    (r"entry_cost", "pair_cost"),
    (r"total_usd_cost", "total_notional_cost"),
    # --- the side/venue axis --------------------------------------------------
    (r"Side", "Venue"),
    (r"side", "venue"),
    # --- remaining bare market vocabulary ------------------------------------
    (r"markets", "strategies"),
    (r"market", "strategy"),
    (r"token", "instrument"),
    (r"wallet", "custody account"),
]

# Literal (non-word-boundary) replacements for string contents and fixtures.
LITERALS = [
    ('"YES"', '"NORTH"'),
    ('"NO"', '"SOUTH"'),
    ('"arb-1"', '"pex-1"'),
    ('"arb"', '"pex"'),
    ('shared_yes_instrument', 'shared_north_instrument'),
    ('shared_yes_token', 'shared_north_instrument'),
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


def main():
    changed = 0
    for path in files():
        try:
            original = path.read_text()
        except UnicodeDecodeError:
            continue
        text = original
        for pattern, repl in PAIRS:
            text = re.sub(rf"\b{pattern}\b", repl, text)
        for old, new in LITERALS:
            text = text.replace(old, new)
        if text != original:
            path.write_text(text)
            changed += 1
            print(f"  rewrote {path.relative_to(ROOT)}")
    print(f"\n{changed} files rewritten")


if __name__ == "__main__":
    main()
