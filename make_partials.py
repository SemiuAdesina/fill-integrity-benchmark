#!/usr/bin/env python3
"""Partial-fix rejection battery.

Each variant takes the *reference solution* and removes exactly one essential
mechanism, producing a plausible near-miss fix. Every one must still score 0.

This is the check that the task cannot be passed by getting the easy half right.
Variants are expressed as literal substitutions against the reference sources so
that a drift in the reference (a rename, a refactor) fails loudly with
PATTERN NOT FOUND rather than silently producing an unmodified variant that
would then "pass" the battery for the wrong reason.
"""
import shutil
import sys
from pathlib import Path

TASK = Path(__file__).resolve().parent / "attributable-fill-detection"
FIXED = TASK / "solution" / "fixed" / "src"
OUT = Path(sys.argv[1] if len(sys.argv) > 1 else "/tmp/afd-partials")

CYCLE = "engine/cycle.rs"
LEDGER = "engine/fill_classifier.rs"
NEUTRAL = "engine/neutralization.rs"

# name -> (description, [(relpath, old, new), ...])
VARIANTS = {
    "P2-no-visibility-rule": (
        "Ledger is correct, but an unconfirmed leg is neutralized instead of "
        "being left alone. An order that has already filled but dropped off the "
        "open-order list gets a second economic order against it.",
        [(CYCLE,
          "            LegVisibility::Unconfirmed => return Ok(()),",
          "            LegVisibility::Unconfirmed => {}")],
    ),
    "P3-feed-only-no-open-orders": (
        "Attribution is built from the fill event feed alone. The open-order "
        "list is read for visibility but never folded into the ledger, so a "
        "withheld or lagging feed stalls completion forever.",
        [(CYCLE,
          "                self.ledger.absorb_open_orders(&open);\n",
          "")],
    ),
    "P4-open-orders-only-no-feed": (
        "Attribution is built from the open-order list alone. A fill that "
        "removes the order from the list before the next poll is never "
        "attributed to anything.",
        [(CYCLE,
          "        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {\n"
          "            self.observed_fill_events += self.ledger.absorb_batch(batch);\n"
          "        }",
          "        if let Ok(batch) = self.fills.poll(&self.ledger.cursor()) {\n"
          "            self.observed_fill_events += batch.events.len();\n"
          "        }")],
    ),
    "P5-retry-under-fresh-client-ref": (
        "A lost acknowledgement is retried, but under a newly minted client "
        "reference, so the venue cannot deduplicate it against the order it "
        "already accepted. Two economic neutralizations for one gap.",
        [(CYCLE,
          "        for record in unconfirmed {\n"
          "            let request = Self::neutralization_request(&record);",
          "        for mut record in unconfirmed {\n"
          "            let stale_ref = record.client_ref.clone();\n"
          "            record.client_ref = self.next_client_ref();\n"
          "            if let Some(slot) = self\n"
          "                .neutralizations\n"
          "                .iter_mut()\n"
          "                .find(|slot| slot.client_ref == stale_ref)\n"
          "            {\n"
          "                slot.client_ref = record.client_ref.clone();\n"
          "            }\n"
          "            let request = Self::neutralization_request(&record);")],
    ),
    "P6-permanent-per-mandate-flag": (
        "Duplicate neutralization is suppressed with a permanent per-mandate "
        "flag. The simple cases pass and then a mandate whose first attempt was "
        "refused is blocked from ever making progress.",
        [(CYCLE,
          "    resting_at_venue: BTreeSet<OrderId>,",
          "    resting_at_venue: BTreeSet<OrderId>,\n"
          "    neutralized_once: BTreeSet<StrategyId>,"),
         (CYCLE,
          "            resting_at_venue: BTreeSet::new(),",
          "            resting_at_venue: BTreeSet::new(),\n"
          "            neutralized_once: BTreeSet::new(),"),
         (CYCLE,
          "    fn close_gap(&mut self, leg: &PendingOrder) -> Result<(), PortError> {\n"
          "        match self.visibility(leg) {",
          "    fn close_gap(&mut self, leg: &PendingOrder) -> Result<(), PortError> {\n"
          "        if self.neutralized_once.contains(&leg.strategy_id) {\n"
          "            return Ok(());\n"
          "        }\n"
          "        match self.visibility(leg) {"),
         (CYCLE,
          "        let request = Self::neutralization_request(&record);\n"
          "        self.neutralizations.push(record);",
          "        let request = Self::neutralization_request(&record);\n"
          "        self.neutralizations.push(record);\n"
          "        self.neutralized_once.insert(leg.strategy_id.clone());")],
    ),
    "P7-neutralize-full-leg-size": (
        "The residual gap is not computed: the neutralization is sized at the "
        "full original leg. A partly filled opposite leg is over-neutralized.",
        [(CYCLE,
          "        let gap = leg.size - self.acquired_for(leg) - self.outstanding_for(leg);",
          "        let gap = leg.size;")],
    ),
    "P8-ignore-outstanding-neutralizations": (
        "The gap subtracts attributed quantity but not neutralizations already "
        "in flight, so an unresolved neutralization is placed a second time on "
        "the next cycle.",
        [(CYCLE,
          "        let gap = leg.size - self.acquired_for(leg) - self.outstanding_for(leg);",
          "        let gap = leg.size - self.acquired_for(leg);")],
    ),
    "P9-client-seq-not-persisted": (
        "Everything is right except the client-reference counter, which resets "
        "on restart. The regenerated reference collides with one the venue "
        "already knows and is deduplicated onto an unrelated order.",
        [(CYCLE,
          "            client_seq: self.client_seq,",
          "            client_seq: 0,")],
    ),
    "P10-ledger-not-persisted": (
        "The ledger is correct in-process but is not written to the snapshot, "
        "so a restart re-attributes every historical fill as new.",
        [(CYCLE,
          "            attributed_fills: self.ledger.entries(),",
          "            attributed_fills: Vec::new(),")],
    ),
}


def build(name, subs):
    dest = OUT / name
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(TASK / "environment", dest)
    # Apply the reference solution first.
    for rel in ["types/snapshot.rs", "engine/fill_classifier.rs",
                "engine/neutralization.rs", "engine/cycle.rs", "engine/mod.rs"]:
        shutil.copy(FIXED / rel, dest / "src" / rel)
    # Then degrade it.
    for rel, old, new in subs:
        path = dest / "src" / rel
        text = path.read_text()
        if old not in text:
            raise SystemExit(f"PATTERN NOT FOUND in {name} / {rel}:\n{old!r}")
        if text.count(old) != 1:
            raise SystemExit(f"PATTERN NOT UNIQUE ({text.count(old)}x) in {name} / {rel}")
        path.write_text(text.replace(old, new))
    return dest


if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for name, (desc, subs) in VARIANTS.items():
        path = build(name, subs)
        print(f"built {name}")
    print(f"\n{len(VARIANTS)} partial-fix variants under {OUT}")
