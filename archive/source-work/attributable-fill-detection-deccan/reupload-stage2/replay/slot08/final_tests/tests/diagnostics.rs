//! Candidate-visible diagnostics for the documented rules.
//!
//! Every check here restates one rule from `/app/spec/venue-semantics.md` or
//! `/app/spec/frozen-contract.md` on synthetic values invented for the check.
//! None of them uses a graded fixture, and none of them is what your work is
//! scored against — the graded suite is separate, larger, and combines these
//! rules in ways a single check cannot. Passing every diagnostic is necessary,
//! not sufficient.
//!
//! Run them with `cargo test --offline --test diagnostics`. Each failure names
//! the rule it broke.
//!
//! This file is yours to change. Add checks as you work out what a rule means.

mod harness;

use harness::*;

use pairing_engine::engine::AttributionLedger;
use pairing_engine::ports::{FillBatch, SubmitError};
use pairing_engine::types::snapshot::Snapshot;
use pairing_engine::types::time::Timestamp;
use pairing_engine::{Engine, EngineConfig, ATTRIBUTION_WINDOW};

const OPEN: i64 = 1_000;

/// One tracked order of `size`, its window opening at `OPEN`.
fn ledger_with(order_id: &str, client_ref: &str, venue_seq: u64, size: i64) -> AttributionLedger {
    let mut ledger = AttributionLedger::new();
    ledger.track(
        client(client_ref),
        Some(order(order_id)),
        Some(venue_seq),
        size,
        at(OPEN),
    );
    ledger
}

fn window_end() -> Timestamp {
    at(OPEN) + ATTRIBUTION_WINDOW
}

/// The attribution window is half-open at its start: `[start, ...`.
#[test]
fn window_start_is_inclusive() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let _ = ledger.absorb_batch(batch(vec![event("o1", "c1", 4, at(OPEN))], vec![], "k1"));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: the attribution window start is inclusive - a fill at exactly \
         window_start belongs to that order"
    );
}

/// The attribution window is half-open at its end: `..., start + window)`.
#[test]
fn window_end_is_exclusive() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let _ = ledger.absorb_batch(batch(vec![event("o1", "c1", 4, window_end())], vec![], "k1"));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: the attribution window end is exclusive - a fill at exactly \
         window_start + ATTRIBUTION_WINDOW is outside the window"
    );
}

/// The instant before the end is still inside the window.
#[test]
fn last_instant_inside_the_window_is_attributed() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let last = Timestamp::from_millis(window_end().as_millis() - 1);
    let _ = ledger.absorb_batch(batch(vec![event("o1", "c1", 4, last)], vec![], "k1"));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: the last instant before window_start + ATTRIBUTION_WINDOW is \
         inside the attribution window"
    );
}

/// `FillEvent::cumulative_filled_qty` is a total, not an increment. The feed may
/// replay it and may deliver it out of order.
#[test]
fn cumulative_fill_quantities_are_a_high_water_mark() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let _ = ledger.absorb_batch(batch(
        vec![
            event("o1", "c1", 4, at(OPEN)),
            event("o1", "c1", 4, at(OPEN + 1)),
            event("o1", "c1", 7, at(OPEN + 2)),
            event("o1", "c1", 5, at(OPEN + 3)),
        ],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        7,
        "FAILED: cumulative_filled_qty is cumulative - redelivered and \
         out-of-order events must not add quantity, only raise it"
    );
}

/// A tracked order that no observation reaches keeps nothing attributed to it.
#[test]
fn an_untracked_order_is_attributed_nothing() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let _ = ledger.absorb_batch(batch(
        vec![event("stranger", "stranger-ref", 9, at(OPEN))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: an observation naming an order this engine is not tracking is \
         attributed to nothing"
    );
}

/// Three orders, one window start, ascending venue sequence: the worked example
/// in `/app/spec/venue-semantics.md`.
fn three_way_block() -> AttributionLedger {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN));
    ledger.track(client("c3"), Some(order("o3")), Some(3), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o3", "o1", "o2"], 37, at(OPEN + 60))],
        "k1",
    ));
    ledger
}

/// A base share is the truncated integer quotient. Nothing rounds up.
#[test]
fn block_shares_are_truncated_never_rounded_up() {
    let ledger = three_way_block();
    assert_eq!(
        ledger.attributed_for(&client("c3")),
        22,
        "FAILED: a block share is (q * outstanding) / basis truncated toward \
         zero - 37*30/50 is 22, not 23"
    );
}

/// The whole divisible total is placed, so the shares plus the remainder units
/// account for every base unit of the block.
#[test]
fn a_block_total_is_fully_allocated() {
    let ledger = three_way_block();
    let total: i64 = ["c1", "c2", "c3"]
        .iter()
        .map(|reference| ledger.attributed_for(&client(reference)))
        .sum();
    assert_eq!(
        total,
        37,
        "FAILED: a block whose total is within the basis is allocated in full - \
         truncated base units are placed, not dropped"
    );
}

/// The remainder goes to the *first* orders in the documented ordering, one unit
/// each — not to the largest order and not to the largest truncated fraction.
#[test]
fn block_remainder_units_go_to_the_first_orders_in_the_ordering() {
    let ledger = three_way_block();
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: each of the first r orders in the block ordering receives one \
         extra base unit"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        7,
        "FAILED: only the first r orders in the block ordering receive an extra \
         base unit"
    );
}

/// Step 2 of the ordering is coarsest field first: an earlier window start comes
/// before a later one whatever the identifiers say.
#[test]
fn the_earlier_attribution_window_sorts_first() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("cz"), Some(order("z-order")), Some(5), 10, at(OPEN));
    ledger.track(client("ca"), Some(order("a-order")), Some(5), 30, at(OPEN + 100));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["a-order", "z-order"], 37, at(OPEN + 200))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("cz")),
        10,
        "FAILED: the block ordering is (window start, venue_seq, order_id) - the \
         earlier window start sorts first even when its order_id sorts last"
    );
}

/// With one window start, `venue_seq` decides, still ahead of `order_id`.
#[test]
fn the_lower_venue_sequence_breaks_a_shared_window_start() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("cz"), Some(order("z-order")), Some(1), 10, at(OPEN));
    ledger.track(client("ca"), Some(order("a-order")), Some(2), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["a-order", "z-order"], 37, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("cz")),
        10,
        "FAILED: with an equal window start the lower venue_seq sorts first, \
         ahead of the order_id comparison"
    );
}

/// Every base unit left by truncation goes to a *different* order, one each,
/// starting from the front of the ordering.
#[test]
fn each_remainder_unit_goes_to_a_different_leading_order() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 1, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 1, at(OPEN));
    ledger.track(client("c3"), Some(order("o3")), Some(3), 1, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o3", "o2", "o1"], 2, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        1,
        "FAILED: the leading order in the block ordering takes one remainder unit"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        1,
        "FAILED: the second order in the block ordering takes the second remainder \
         unit - the remainder is not handed to one order in a lump"
    );
    assert_eq!(
        ledger.attributed_for(&client("c3")),
        0,
        "FAILED: only the first r orders receive a remainder unit"
    );
}

/// Quantity above the basis is not attributable to anything this engine tracks,
/// so it is dropped rather than spread around.
#[test]
fn block_quantity_above_the_basis_is_dropped() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1", "o2"], 25, at(OPEN + 60))],
        "k1",
    ));
    let total: i64 = ["c1", "c2"]
        .iter()
        .map(|reference| ledger.attributed_for(&client(reference)))
        .sum();
    assert_eq!(
        total, 20,
        "FAILED: q is min(block_qty, basis) - the 5 base units above the basis were \
         not matched against anything this engine is tracking"
    );
}

/// A covered order whose window does not contain the block's match time takes
/// nothing from the division, and is closed by the block all the same.
#[test]
fn a_covered_order_outside_its_window_takes_nothing_and_is_closed() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN + 2_000));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 10, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        10,
        "FAILED: the eligible set is the covered orders whose window contains the \
         block's match time, so the whole block goes to o1"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: a covered order outside its own attribution window takes nothing"
    );
    // o2's own window is open now, but the venue closed o2 when it named it.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2"], 5, at(OPEN + 2_060))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: eligibility decides what an order is allocated, not whether the \
         venue closed it - a named order is finalized either way"
    );
}

/// The division is invariant. The venue builds `covers` in whatever order it
/// likes, and no order's share may depend on that; the whole divisible total is
/// placed, and no order takes more than it had outstanding.
#[test]
fn the_division_does_not_depend_on_the_order_of_covers() {
    // A small deterministic generator: no crate, no wall clock.
    let mut seed: u64 = 0x5eed_1234;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize
    };

    for _ in 0..64 {
        let count = 2 + next() % 4;
        let sizes: Vec<i64> = (0..count).map(|_| 1 + next() as i64 % 20).collect();
        let seqs: Vec<u64> = (0..count).map(|_| next() as u64 % 3).collect();
        let starts: Vec<i64> = (0..count).map(|_| (next() % 3) as i64 * 100).collect();
        let block_qty = 1 + next() as i64 % 60;
        let ids: Vec<String> = (0..count).map(|i| format!("o{i}")).collect();

        let build = |permutation: &[usize]| -> AttributionLedger {
            let mut ledger = AttributionLedger::new();
            for position in 0..count {
                ledger.track(
                    client(&format!("c{position}")),
                    Some(order(&ids[position])),
                    Some(seqs[position]),
                    sizes[position],
                    at(OPEN + starts[position]),
                );
            }
            let covers: Vec<&str> = permutation.iter().map(|position| ids[*position].as_str()).collect();
            let _ = ledger.absorb_batch(batch(
                vec![],
                vec![block("b1", &covers, block_qty, at(OPEN + 10))],
                "k1",
            ));
            ledger
        };

        let forward: Vec<usize> = (0..count).collect();
        let mut reversed: Vec<usize> = forward.clone();
        reversed.reverse();
        let in_order = build(&forward);
        let reversed_order = build(&reversed);

        let shares: Vec<i64> = (0..count)
            .map(|position| in_order.attributed_for(&client(&format!("c{position}"))))
            .collect();
        let reversed_shares: Vec<i64> = (0..count)
            .map(|position| reversed_order.attributed_for(&client(&format!("c{position}"))))
            .collect();
        assert_eq!(
            shares, reversed_shares,
            "FAILED: the iteration order of the covered orders must never decide an \
             attribution outcome - sizes {sizes:?}, sequences {seqs:?}, block {block_qty}"
        );

        // Everything whose window contains the match is eligible, so those
        // orders' outstanding quantities are the basis.
        let basis: i64 = starts
            .iter()
            .zip(&sizes)
            .filter(|(start, _)| **start <= 10)
            .map(|(_, size)| *size)
            .sum();
        let total: i64 = shares.iter().sum();
        assert_eq!(
            total,
            if block_qty < basis { block_qty } else { basis },
            "FAILED: the divisible total is min(block_qty, basis) and is placed in \
             full - sizes {sizes:?}, block {block_qty}, basis {basis}"
        );
        for (position, share) in shares.iter().enumerate() {
            assert!(
                *share <= sizes[position],
                "FAILED: no order is allocated more than it had outstanding"
            );
        }
    }
}

/// `block_ref` is the idempotency key for a block.
#[test]
fn a_redelivered_block_is_counted_once() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let replayed = block("b1", &["o1"], 4, at(OPEN + 60));
    let _ = ledger.absorb_batch(batch(vec![], vec![replayed.clone()], "k1"));
    let _ = ledger.absorb_batch(batch(vec![], vec![replayed], "k2"));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: block_ref is the block idempotency key - the same block_ref is \
         the same match however many times the feed delivers it"
    );
}

/// The basis is *current outstanding* quantity, not the order's original size.
/// Dividing over the size would give c1 5 and c2 15 here.
#[test]
fn a_block_divides_over_outstanding_quantity_not_original_size() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    // o2 has already acquired 12 directly, so only 18 of it is outstanding.
    let _ = ledger.absorb_batch(batch(
        vec![event("o2", "c2", 12, at(OPEN + 10))],
        vec![],
        "k1",
    ));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 20, at(OPEN + 60))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: the basis is the eligible orders' outstanding quantity - 20 split \
         over 10 and 18 gives o1 7, plus the one remaining base unit"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        24,
        "FAILED: the basis is outstanding quantity, not size - o2's share is \
         20*18/28 = 12 on top of the 12 it already held, not 20*30/28"
    );
}

/// A block finalizes every tracked order it covers, and a finalized order
/// contributes no outstanding quantity to a later block's basis. Dividing the
/// later block over both would give c1 7 and c2 7 here.
#[test]
fn a_finalized_order_contributes_no_outstanding_to_a_later_block() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1"], 4, at(OPEN + 60))],
        "k1",
    ));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o1", "o2"], 10, at(OPEN + 120))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: an order a block has finalized contributes no outstanding quantity \
         to a later block's basis - the venue closed it"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        10,
        "FAILED: the later block's basis is what the unfinalized covered orders have \
         outstanding, so the whole block goes to o2"
    );
}

/// A covered order that the division skipped — here because it had no
/// outstanding quantity left — is still finalized by the block.
#[test]
fn a_covered_order_the_division_skipped_is_still_finalized() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN));
    // o2 is already complete, so it is ineligible for the block's division.
    let _ = ledger.absorb_batch(batch(
        vec![event("o2", "c2", 10, at(OPEN + 10))],
        vec![],
        "k1",
    ));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 5, at(OPEN + 60))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        5,
        "FAILED: an ineligible covered order takes nothing from the division"
    );
    // The venue closed o2 by naming it, so a later block gives it nothing, and
    // so does anything else: it will never acquire another base unit.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2"], 3, at(OPEN + 120))],
        "k3",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        10,
        "FAILED: a block finalizes every tracked order it covers, including one the \
         division allocated nothing to"
    );
}

/// `open_orders` entries carry no match timestamp, so no window is tested
/// against them, and `filled_qty` is a total like a fill event's.
#[test]
fn the_open_order_list_is_point_in_time_truth() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    ledger.absorb_open_orders(&[open_order("o1", "c1", 1, 10, 6)]);
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        6,
        "FAILED: an open_orders entry carries no timestamp, so the attribution \
         window does not apply to it"
    );
    ledger.absorb_open_orders(&[open_order("o1", "c1", 1, 10, 4)]);
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        6,
        "FAILED: filled_qty on an open order is a total, so it raises what is \
         attributed and never lowers it"
    );
}

// ---------------------------------------------------------------------------
// Whole-cycle diagnostics, driven only through `Engine::run_cycle` and the ports.
// ---------------------------------------------------------------------------

/// One mandate of 10 a side, both legs affordable, expiring far enough out that
/// nothing is retired while the diagnostic runs.
struct Rig {
    clock: FakeClock,
    strategies: FakeStrategies,
    inventory: FakeInventory,
    gateway: FakeGateway,
    fills: FakeFills,
    store: FakeStore,
}

impl Rig {
    fn new(batches: Vec<FillBatch>) -> Self {
        Self {
            clock: FakeClock::new(at(OPEN)),
            strategies: FakeStrategies::new(vec![candidate("m1", 10, at(10_000))]),
            inventory: FakeInventory::new(1_000.0),
            gateway: FakeGateway::new(),
            fills: FakeFills::new(batches),
            store: FakeStore::empty(),
        }
    }

    fn engine(&self) -> Engine<'_> {
        Engine::new(
            EngineConfig::default(),
            &self.strategies,
            &self.inventory,
            &self.gateway,
            &self.fills,
            &self.store,
            &self.clock,
        )
    }
}

/// The north leg of `m1` is filled in full; the south leg is not.
fn north_filled() -> Vec<FillBatch> {
    vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
    ]
}

fn attributed_in(snapshot: &Snapshot, client_ref: &str) -> i64 {
    snapshot
        .submissions
        .iter()
        .find(|record| record.client_ref == client(client_ref))
        .map(|record| record.attributed())
        .expect("FAILED: every order the engine submits is tracked in the snapshot")
}

/// An order missing from `open_orders` that this engine never cancelled is
/// unconfirmed. It may still be resting and may still fill, so nothing may be
/// concluded from its absence.
#[test]
fn an_absent_order_this_engine_did_not_cancel_is_not_withdrawn() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: an order absent from open_orders that this engine did not cancel \
         is unconfirmed, not withdrawn - it may still fill, so no corrective \
         order may be sent for it"
    );
    assert!(
        rig.gateway.cancelled.borrow().is_empty(),
        "FAILED: an unconfirmed order is not something to cancel - the venue may \
         never have accepted it"
    );
}

/// Drive the rig to the point where the south leg has been cancelled and one
/// corrective order has been sent for it. `submission` 3 is that corrective.
fn engine_after_first_corrective(rig: &Rig, engine: &mut Engine<'_>) {
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    rig.gateway.set_open(vec![south]);

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    rig.gateway.set_open(vec![]);
}

/// Quantity already working on a corrective order is not exposure to cover
/// again. Sending a second order for it acquires the quantity twice.
#[test]
fn quantity_already_working_is_not_neutralized_twice() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: a cancelled leg is covered for exactly the quantity it is short"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: quantity still working on an earlier corrective order is not \
         exposure - it must not be neutralized a second time"
    );
}

/// A corrective order the venue has closed leaves a real gap behind: it is no
/// longer working, so the exposure it was meant to cover is corrected again.
#[test]
fn a_finalized_corrective_order_leaves_its_gap_open() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);
    assert_eq!(rig.gateway.corrective_sizes(), vec![10]);

    // The corrective is submission 3, V-103 under pex-3. A block fills 4 of its
    // 10 and the venue cancels the rest, so 6 is genuinely still missing.
    rig.fills.batches.borrow_mut().push(batch(
        vec![],
        vec![block("bX", &["V-103"], 4, at(OPEN + 120))],
        "k2",
    ));
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 6],
        "FAILED: a corrective order the venue finalized contributes no working \
         quantity, so the gap it leaves behind is neutralized - and a mandate \
         already corrected once must stay able to make progress"
    );
}

/// A leg the venue closed by naming it in a block is known to be short, unlike
/// one whose absence from the live list proves nothing.
#[test]
fn a_leg_closed_by_a_block_is_neutralized_for_what_it_is_short() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![block("bY", &["V-102"], 3, at(OPEN + 60))], "k2"),
    ]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: at cycle 2 the south leg is merely unconfirmed - absent from \
         open_orders for a reason this engine cannot see"
    );

    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![7],
        "FAILED: a block finalizes the order it names, so the 3 of 10 it acquired is \
         attributed to the leg and the 7 the venue cancelled is neutralized"
    );
}

/// A lost acknowledgement is not a rejection. The order may exist at the venue,
/// and `client_ref` is the venue's idempotency key, so the retry has to reuse it.
#[test]
fn a_lost_acknowledgement_is_retried_under_the_same_client_ref() {
    let rig = Rig::new(north_filled());
    rig.gateway
        .fail_on(3, SubmitError::AcknowledgementLost);
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    let refs = rig.gateway.corrective_refs();
    assert_eq!(
        refs.len(),
        2,
        "FAILED: a corrective order whose acknowledgement was lost is retried \
         once, not abandoned and not retried repeatedly in one cycle"
    );
    assert_eq!(
        refs[1], refs[0],
        "FAILED: client_ref is the venue's idempotency key - retrying a lost \
         acknowledgement under a fresh client_ref asks for a second order"
    );
}

/// The venue recycles order identifiers once a window has closed, and the feed
/// replays for far longer than a window lasts. An observation naming a recycled
/// identifier is evidence about a different order; the client reference the
/// engine chose is what tells them apart.
#[test]
fn a_recycled_identifier_is_resolved_by_its_client_reference() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o-rec")), Some(1), 10, at(OPEN));
    // A later order of this engine's, whose window opens after c1's has closed.
    ledger.track(client("c9"), None, None, 10, at(OPEN + 2_000));

    // The venue handed o-rec to the later order, and echoes its client reference.
    ledger.absorb_open_orders(&[open_order("o-rec", "c9", 9, 10, 5)]);

    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: a submission is identified by the client_ref the engine chose - \
         quantity belonging to a later order that inherited the identifier is not \
         evidence about the earlier one"
    );
    assert_eq!(
        ledger.attributed_for(&client("c9")),
        5,
        "FAILED: the client_ref a live order echoes is what resolves it, and the \
         id this engine had not been told yet is learned from it"
    );
}

/// A replayed event naming an identifier this engine tracked long ago, under a
/// client reference it does not know, falls outside the original window.
#[test]
fn a_replayed_event_outside_the_original_window_is_not_attributed() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o-rec")), Some(1), 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![event("o-rec", "stranger", 7, at(OPEN + 2_000))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: an observation timestamped outside an order's window names an \
         identifier that is no longer that order - counting it is a misattribution"
    );
}

/// The feed is at-least-once. Handing the engine the same evidence again and
/// again, cycle after cycle, must not accumulate attribution, re-open a settled
/// corrective, or grow the number of economic orders.
#[test]
fn replaying_every_observation_for_many_cycles_changes_nothing() {
    let observations = || {
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-102"], 3, at(OPEN))],
            "k1",
        )
    };
    let rig = Rig::new(vec![batch(vec![], vec![], "k0"), observations()]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    for step in 1..6 {
        rig.fills.batches.borrow_mut().push(observations());
        rig.clock.set(at(OPEN + 60 * step));
        engine.run_cycle().expect("later cycle");
    }

    assert!(
        rig.gateway.corrective_sizes().len() <= 1,
        "FAILED: quantity still working on a committed corrective order is not \
         exposure - redelivered evidence must not send it again, saw {:?}",
        rig.gateway.corrective_sizes()
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![7],
        "FAILED: the north leg filled for 10, the block left the south leg 3 of 10, \
         so exactly the missing 7 is neutralized - once"
    );
    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        10,
        "FAILED: cumulative_filled_qty is a high-water mark - replaying it for five \
         cycles must not raise it"
    );
    assert_eq!(
        attributed_in(&saved, "pex-2"),
        3,
        "FAILED: block_ref makes a block idempotent - replaying it for five cycles \
         must not count it again"
    );
}

/// An unacknowledged corrective order is durable, together with the reference a
/// retry depends on. A restart that forgot either would place the exposure twice.
#[test]
fn an_unacknowledged_corrective_survives_a_restart_with_its_identity() {
    let rig = Rig::new(north_filled());
    rig.gateway.fail_on(3, SubmitError::AcknowledgementLost);
    {
        let mut engine = rig.engine();
        engine_after_first_corrective(&rig, &mut engine);
    }

    let saved = rig.store.snapshot();
    assert!(
        saved
            .neutralizations
            .iter()
            .any(|record| record.client_ref == client("pex-3") && record.order_id.is_none()),
        "FAILED: a corrective order whose acknowledgement was lost is durable \
         state - the venue may hold it and this engine cannot see it"
    );

    // A second engine over the same store: a restart.
    let mut engine = rig.engine();
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4 after restart");

    let refs = rig.gateway.corrective_refs();
    assert_eq!(
        refs.len(),
        2,
        "FAILED: an unacknowledged corrective is retried after a restart rather \
         than abandoned"
    );
    assert_eq!(
        refs[1], refs[0],
        "FAILED: the client reference a retry depends on is durable state - a \
         reset counter would regenerate a reference the venue already knows"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 10],
        "FAILED: the retry is the same economic order under the same reference, \
         not a second order for the same exposure"
    );
}

/// Attribution state, the feed cursor and the absorbed block references are all
/// durable, and a replay after a restart must change nothing.
#[test]
fn attribution_state_survives_a_restart_without_double_counting() {
    let replayed = || {
        batch(
            vec![event("V-101", "pex-1", 6, at(OPEN))],
            vec![block("bA", &["V-102"], 3, at(OPEN))],
            "k1",
        )
    };
    let rig = Rig::new(vec![batch(vec![], vec![], "k0"), replayed()]);

    {
        let mut engine = rig.engine();
        engine.run_cycle().expect("cycle 1");
        rig.clock.set(at(OPEN + 60));
        engine.run_cycle().expect("cycle 2");
    }

    let saved = rig.store.snapshot();
    assert_eq!(
        saved.fill_cursor.as_str(),
        "k1",
        "FAILED: the fill-feed cursor is durable state and belongs in the snapshot"
    );
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        6,
        "FAILED: per-order attribution state is durable state and belongs in the \
         snapshot"
    );
    assert!(
        saved.absorbed_blocks.iter().any(|reference| reference.as_str() == "bA"),
        "FAILED: the block references already absorbed are durable state - a \
         block replayed after a restart must still be counted once"
    );

    // A second engine over the same store: a restart, with the feed replaying.
    rig.fills.batches.borrow_mut().push(replayed());
    {
        let mut engine = rig.engine();
        rig.clock.set(at(OPEN + 120));
        engine.run_cycle().expect("cycle 3 after restart");
    }

    assert_eq!(
        rig.fills.polled_with.borrow()[2],
        "k1",
        "FAILED: after a restart the feed is polled from the saved cursor, not \
         from the beginning"
    );
    let restarted = rig.store.snapshot();
    assert_eq!(
        attributed_in(&restarted, "pex-1"),
        6,
        "FAILED: a replayed fill event carries the same cumulative quantity and \
         must not add to what is already attributed"
    );
    assert_eq!(
        attributed_in(&restarted, "pex-2"),
        3,
        "FAILED: a block replayed after a restart is the same match and must be \
         counted once"
    );
}
