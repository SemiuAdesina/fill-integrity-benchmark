#!/usr/bin/env python3
"""Generate realistic partial implementations from the V8 reference solution.

Each partial removes exactly one mechanism a correct implementation needs, or
substitutes one plausible-but-wrong reading of the venue rules. They are written
under /tmp/fillv8-validate/partials/<name>/ as a src/ overlay.
"""
import pathlib
import shutil
import sys

REF = pathlib.Path(
    "/Users/macbok/Downloads/benchmark-suite-solvability-repair/fill-v8-work/build/"
    "attributable-fill-detection/solution/fixed"
)
OUT = pathlib.Path("/tmp/fillv8-validate/partials")

FC = "src/engine/fill_classifier.rs"
CY = "src/engine/cycle.rs"

EDITS = {
    "p02_event_feed_only": (FC, """            if open_order.filled_qty > record.direct_filled {
                record.direct_filled = open_order.filled_qty;
            }""", """            let _ = open_order.filled_qty;"""),
    "p03_open_order_list_only": (FC, """        for event in batch.events {
            self.absorb_event(event);
        }""", """        for event in batch.events {
            let _ = event;
        }"""),
    "p04_absence_means_withdrawn": (CY, """        if self.resting_at_venue.contains(&leg.order_id) {
            LegVisibility::Resting
        } else if self.replaced.contains(&leg.order_id) {
            LegVisibility::Withdrawn
        } else {
            LegVisibility::Unconfirmed
        }""", """        if self.resting_at_venue.contains(&leg.order_id) {
            LegVisibility::Resting
        } else {
            LegVisibility::Withdrawn
        }"""),
    "p05_fresh_client_ref_on_retry": (CY, """            Err(SubmitError::AcknowledgementLost) => {
                self.ledger.track(client_ref.clone(), None, None, size, now);
            }
            Err(_) => {""", """            Err(_) => {"""),
    "p06_attribution_not_persisted": (CY, """            submissions: self.ledger.submissions(),
            absorbed_blocks: self.ledger.absorbed_blocks(),""",
        """            submissions: Vec::new(),
            absorbed_blocks: Vec::new(),"""),
    "p07_inclusive_window_end": (FC,
        "at >= record.window_start && at < record.window_start + ATTRIBUTION_WINDOW",
        "at >= record.window_start && at <= record.window_start + ATTRIBUTION_WINDOW"),
    "p08_summed_cumulative_events": (FC, """        if event.cumulative_filled_qty > record.direct_filled {
            record.direct_filled = event.cumulative_filled_qty;
        }""", """        record.direct_filled += event.cumulative_filled_qty;"""),
    "p09_float_rounded_block_shares": (FC,
        "let share = proportional_share(divisible, *outstanding, basis);",
        "let share = ((divisible as f64) * (*outstanding as f64) / (basis as f64))"
        ".round() as Qty;"),
    "p10_remainder_to_largest_order": (FC, """        let mut remainder = divisible - assigned;
        for share in shares.iter_mut() {
            if remainder <= 0 {
                break;
            }
            *share += 1;
            remainder -= 1;
        }""", """        let remainder = divisible - assigned;
        if remainder > 0 {
            let mut best = 0usize;
            for (position, (_, _, outstanding)) in eligible.iter().enumerate() {
                if *outstanding > eligible[best].2 {
                    best = position;
                }
            }
            shares[best] += remainder;
        }"""),
    "p11_no_block_ref_dedup": (FC, """        if !self.absorbed_blocks.insert(block.block_ref.clone()) {
            return;
        }""", """        self.absorbed_blocks.insert(block.block_ref.clone());"""),
    "p12_block_divides_over_order_size": (FC, """            let outstanding = record.outstanding();
            if outstanding <= 0 {
                continue;
            }""", """            let outstanding = record.size;
            if record.outstanding() <= 0 {
                continue;
            }"""),
}


def main() -> int:
    if OUT.exists():
        shutil.rmtree(OUT)
    for name, (rel, old, new) in EDITS.items():
        target = OUT / name
        target.mkdir(parents=True)
        text = (REF / rel).read_text()
        if text.count(old) != 1:
            print(f"{name}: pattern appears {text.count(old)} times in {rel}")
            return 1
        (target / "patched.rs").write_text(text.replace(old, new))
        (target / "path.txt").write_text(rel)
        print(f"{name}: patched {rel}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
