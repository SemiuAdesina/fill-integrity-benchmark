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

/// The eligible set's basis is *current* outstanding quantity, so quantity
/// already attributed to an order is not divided against again. Here the earlier
/// attribution came from a fill event, which leaves the order open.
#[test]
fn a_block_divides_over_what_is_still_outstanding() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c1", 2, at(OPEN + 10))],
        vec![],
        "k1",
    ));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 30, at(OPEN + 60))],
        "k2",
    ));
    // Basis is 8 + 30 = 38, so the shares are 30*8/38 = 6 and 30*30/38 = 23 and
    // the one leftover base unit goes to the first order: 6 + 1.
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        9,
        "FAILED: the eligible set's basis is current outstanding quantity - an \
         earlier partial fill reduces what a later block divides against"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        23,
        "FAILED: the eligible set's basis is current outstanding quantity - an \
         earlier partial fill reduces what a later block divides against"
    );
}

/// A block closes every order it names, so a later block naming the same orders
/// has an empty eligible set and attributes nothing to them. Finality is a
/// statement about the venue, not about the arithmetic: the venue cancelled
/// whatever the first block did not fill.
#[test]
fn a_later_block_naming_an_already_finalized_order_allocates_nothing() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 30, at(OPEN + 60))],
        "k1",
    ));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2", "o1"], 5, at(OPEN + 120))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: a block is final for every order it names - a later block naming \
         an order the earlier block closed allocates it nothing"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        22,
        "FAILED: a block is final for every order it names - a later block naming \
         an order the earlier block closed allocates it nothing"
    );
}

/// An order the block named but the division skipped is closed just the same:
/// eligibility decides what an order is allocated, not whether the venue closed
/// it. Here the block's match time falls outside the second order's window.
#[test]
fn a_block_finalizes_a_covered_order_it_did_not_allocate_to() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN + 5_000));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 4, at(OPEN + 60))],
        "k1",
    ));
    // o2 is outside its window at OPEN + 60, so it is not eligible and takes
    // nothing; the whole block lands on o1.
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: an ineligible covered order takes no share of the block"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: an ineligible covered order takes no share of the block"
    );
    // But it is closed by the venue all the same, so it can never take one.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2"], 9, at(OPEN + 5_100))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: a block is final for every order it names, including one the \
         division allocated nothing to"
    );
}

/// Two mandates may name the same instrument identifier. A block covering one
/// mandate's order is matched against that order alone, so none of it can
/// complete the other mandate's leg.
#[test]
fn a_block_covering_one_mandate_does_not_reach_another() {
    let mut ledger = AttributionLedger::new();
    // Mandate A and mandate B, both working the same instrument on North.
    ledger.track(client("a-north"), Some(order("oA")), Some(1), 10, at(OPEN));
    ledger.track(client("b-north"), Some(order("oB")), Some(2), 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["oA"], 10, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("a-north")),
        10,
        "FAILED: a block is divided over the orders it names, and the whole of it \
         belongs to the one covered order"
    );
    assert_eq!(
        ledger.attributed_for(&client("b-north")),
        0,
        "FAILED: two mandates may share an instrument identifier, and quantity \
         acquired by one must not complete the other - a block naming one order \
         says nothing about its neighbour"
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

/// A leg the venue closed is corrected for exactly what the block left behind.
///
/// This one turns on three rules together: a block is final for the order it
/// names, a finalized order contributes no still-working quantity, and the
/// residual is the size less what is attributed to that leg.
#[test]
fn a_block_closed_leg_is_neutralized_for_what_the_block_left() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-102"], 4, at(OPEN))],
            "k1",
        ),
    ]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![6],
        "FAILED: a leg the venue has closed is corrected for exactly the quantity \
         the block left - the block closed the order and cancelled the rest, so \
         those 6 units are a real exposure"
    );
}

/// A corrective order the venue has closed leaves a real gap. It contributes
/// nothing still working, whatever it managed to fill, so the remainder is
/// neutralized again and the mandate keeps making progress.
#[test]
fn a_closed_corrective_leaves_its_gap_open() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        batch(vec![], vec![block("bB", &["V-103"], 4, at(OPEN + 120))], "k3"),
    ]);
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: a cancelled leg is covered for exactly the quantity it is short"
    );

    // Cycle 4 observes the block that closed the first corrective having filled
    // 4 of its 10. Nothing of it is still working, so 6 remain to cover.
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 6],
        "FAILED: a corrective order the venue has closed contributes nothing still \
         working, so the gap it leaves behind is neutralized - and only the gap"
    );
}

/// A rejection is not a lost acknowledgement: nothing was created, the exposure
/// is still open, and a later cycle tries again. The retry is a fresh attempt
/// rather than a resubmission, because there is no order to land on.
#[test]
fn a_rejected_corrective_leaves_the_exposure_open_to_a_later_attempt() {
    let rig = Rig::new(north_filled());
    rig.gateway.fail_on(3, SubmitError::Rejected("rejected".into()));
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);

    // The attempt failed, but it is not retried again inside the same cycle.
    assert_eq!(
        rig.gateway.submitted.borrow().len(),
        3,
        "FAILED: a rejected attempt is not followed by a second economic order in \
         the same cycle"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    let refs = rig.gateway.corrective_refs();
    assert_eq!(
        refs.len(),
        2,
        "FAILED: a rejected correction leaves the exposure open, so a later cycle \
         recomputes it and tries again - a mandate is not latched shut by one \
         failed attempt"
    );
    assert_ne!(
        refs[1], refs[0],
        "FAILED: a rejection created nothing, so the next attempt is a fresh order \
         under a fresh client reference rather than a retry of the original"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 10],
        "FAILED: nothing was created by the rejected attempt, so the exposure is \
         still the full quantity the leg is short"
    );
}

/// Correcting a resting leg is a sequence, not an instant: one cycle marks it,
/// the next cancels it and covers what remains. Nothing is cancelled and nothing
/// is sent on the cycle that first notices.
#[test]
fn a_resting_leg_is_cancelled_before_anything_is_covered() {
    let rig = Rig::new(north_filled());
    let south = open_order("V-102", "pex-2", 102, 10, 0);
    rig.gateway.set_open(vec![south]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert!(
        rig.gateway.cancelled.borrow().is_empty() && rig.gateway.corrective_sizes().is_empty(),
        "FAILED: a resting order is not treated as gone merely because a correction \
         has become necessary - it is moved into closure first"
    );

    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    assert_eq!(
        rig.gateway.cancelled.borrow().len(),
        1,
        "FAILED: the resting leg is cancelled before the corrective order is sent"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: once the cancellation boundary has passed the residual is covered"
    );
}

/// A correction the venue never acknowledged survives a restart with the client
/// reference a retry depends on. A reset reference would ask for a second order.
#[test]
fn an_unacknowledged_correction_survives_a_restart() {
    let rig = Rig::new(north_filled());
    rig.gateway.fail_on(3, SubmitError::AcknowledgementLost);
    {
        let mut engine = rig.engine();
        engine_after_first_corrective(&rig, &mut engine);
    }

    let saved = rig.store.snapshot();
    assert_eq!(
        saved.neutralizations.len(),
        1,
        "FAILED: committed corrections, including ones the venue never \
         acknowledged, are durable state"
    );
    assert!(
        saved.neutralizations[0].order_id.is_none(),
        "FAILED: an unacknowledged correction stays unacknowledged across a restart"
    );

    // A restart, and the retry has to land on the same order.
    let mut engine = rig.engine();
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4 after restart");

    let refs = rig.gateway.corrective_refs();
    assert_eq!(
        refs.len(),
        2,
        "FAILED: an unacknowledged correction is retried after a restart"
    );
    assert_eq!(
        refs[1], refs[0],
        "FAILED: client_ref is the venue's idempotency key and survives a restart - \
         retrying under a fresh reference asks for a second economic order"
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
