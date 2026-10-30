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
use pairing_engine::types::ids::InstrumentId;
use pairing_engine::types::snapshot::Snapshot;
use pairing_engine::types::strategy::StrategyCandidate;
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

/// A block closes every order it names, so a later block naming the same orders
/// finds no outstanding quantity left to divide against and allocates nothing.
///
/// This is the interaction a "residual division" reading gets wrong: eligibility
/// decides what an order is *allocated*, it does not decide whether the venue
/// closed it. `reconciliation-contract.md`: a finalized order "contributes no
/// outstanding quantity to a later block's basis".
#[test]
fn a_block_finalizes_every_order_it_covers() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    // basis 40, q 30: shares 7 and 22, the one remainder unit to o1.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 30, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: the first division is 30 over outstanding (10, 30) - 7 and 22, \
         with the remainder unit on the first order in the ordering"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        22,
        "FAILED: the first division is 30 over outstanding (10, 30) - 7 and 22"
    );

    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2", "o1"], 5, at(OPEN + 120))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: a block finalizes every order it covers, so a finalized order \
         contributes no outstanding quantity to a later block's basis"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        22,
        "FAILED: a block finalizes every order it covers, so a finalized order \
         contributes no outstanding quantity to a later block's basis"
    );
}

/// A later block still divides, over the orders the earlier block did not name:
/// the basis is current outstanding quantity at the later block's match time.
#[test]
fn a_later_block_divides_over_what_is_still_outstanding() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    ledger.track(client("c3"), Some(order("o3")), Some(3), 10, at(OPEN + 60));
    // bA closes o1 and o2, dividing 30 over outstanding (10, 30): 8 and 22.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 30, at(OPEN + 60))],
        "k1",
    ));
    // bB names o2, which bA closed, and o3, which nothing has closed.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2", "o3"], 5, at(OPEN + 120))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        22,
        "FAILED: a finalized order contributes no outstanding quantity to a later \
         block's basis"
    );
    assert_eq!(
        ledger.attributed_for(&client("c3")),
        5,
        "FAILED: the eligible set's basis is current outstanding quantity, so a \
         later block divides over what is still outstanding"
    );
}

/// An order the block named but the division skipped is closed by the venue just
/// the same. Here the block's match time falls outside o2's window, so o2 is
/// allocated nothing — and acquires nothing afterwards either.
#[test]
fn a_block_finalizes_a_covered_order_the_division_skipped() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN + 200));
    // o1 is eligible with outstanding 10; o2's window has not opened yet. The
    // block is 25, over a basis of 10, so 15 base units are dropped.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1", "o2"], 25, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        10,
        "FAILED: quantity above the basis is not attributable to any tracked \
         order, so it is dropped rather than spread"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: an order whose attribution window does not contain the block's \
         match time is allocated nothing"
    );

    // Well inside o2's own window now, but the block closed o2 already.
    let _ = ledger.absorb_batch(batch(
        vec![event("o2", "c2", 7, at(OPEN + 300))],
        vec![],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: a block finalizes every order it covers, including one the \
         division skipped - a finalized order never acquires another base unit"
    );
}

/// An order the block named that had no outstanding quantity left is closed too,
/// so it takes nothing from a later block.
#[test]
fn a_block_finalizes_a_covered_order_with_nothing_outstanding() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN));
    // o2 has already filled in full, so it is not eligible.
    let _ = ledger.absorb_batch(batch(vec![event("o2", "c2", 10, at(OPEN))], vec![], "k1"));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1", "o2"], 10, at(OPEN + 60))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        10,
        "FAILED: the basis is the outstanding quantity of the eligible orders only"
    );
    // o2 was closed with nothing outstanding, so it stays exactly as it was.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b2", &["o1", "o2"], 4, at(OPEN + 120))],
        "k3",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        10,
        "FAILED: a block finalizes every order it covers, including one that had \
         no outstanding quantity to be eligible with"
    );
}

/// An order id repeated in `covers` is one order, not two.
#[test]
fn an_order_named_twice_in_one_block_is_allocated_once() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1", "o2", "o1"], 10, at(OPEN + 60))],
        "k1",
    ));
    let total: i64 = ["c1", "c2"]
        .iter()
        .map(|reference| ledger.attributed_for(&client(reference)))
        .sum();
    assert_eq!(
        total, 10,
        "FAILED: an order named twice in one block is one order - it is allocated \
         one share, not two"
    );
}

/// An observation is matched to a submission by `client_ref` first: it is the
/// only key the engine always knows, and an order the venue never acknowledged
/// has no id of its own yet.
#[test]
fn an_observation_is_matched_by_client_ref_before_order_id() {
    let mut ledger = AttributionLedger::new();
    // A submission whose acknowledgement was lost: no order id, no venue seq.
    ledger.track(client("c1"), None, None, 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(vec![event("late-id", "c1", 4, at(OPEN))], vec![], "k1"));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: client_ref is the key the engine always knows, so an observation \
         is attributed by it - this is how a lost acknowledgement is recovered"
    );
    // The venue's id is learned from that observation.
    let _ = ledger.absorb_batch(batch(vec![event("late-id", "c1", 6, at(OPEN + 1))], vec![], "k2"));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        6,
        "FAILED: once the venue names an order id for a client_ref, either key \
         resolves to the same record"
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
        Self::with(vec![candidate("m1", 10, at(10_000))], batches)
    }

    fn with(candidates: Vec<StrategyCandidate>, batches: Vec<FillBatch>) -> Self {
        Self {
            clock: FakeClock::new(at(OPEN)),
            strategies: FakeStrategies::new(candidates),
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

// ---------------------------------------------------------------------------
// Interactions: two or three rules at once, driven through `run_cycle`.
// ---------------------------------------------------------------------------

/// A block closes the leg it names, so the leg's state is known exactly even
/// though it is gone from `open_orders`: the residual is covered, and the
/// momentarily-unknown rule does not apply to it.
#[test]
fn a_block_closed_leg_is_neutralized_for_what_it_missed() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("b1", &["V-102"], 4, at(OPEN))],
            "k1",
        ),
    ]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![6],
        "FAILED: a block finalizes the order it names, so its state is known - the \
         corrective covers the residual (10 - 4), and the unconfirmed rule does \
         not block it"
    );
}

/// A corrective a block has finalized contributes nothing as still working, so
/// the gap it leaves behind is covered again - and only the gap.
#[test]
fn a_finalized_corrective_leaves_a_gap_that_is_covered_again() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        // The corrective is the third submission, accepted in cycle three, so its
        // window opens at OPEN + 120 and this match is inside it.
        batch(vec![], vec![block("b1", &["V-103"], 4, at(OPEN + 180))], "k3"),
    ]);
    let mut engine = rig.engine();

    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    rig.gateway.set_open(vec![]);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: the first corrective covers the whole residual"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 6],
        "FAILED: a finalized corrective contributes nothing as still working, \
         whatever it filled - the gap it leaves behind is real, and is covered \
         once and only once"
    );
}

/// A rejection creates nothing, so the exposure stays open and a later cycle
/// tries again rather than latching the mandate shut.
#[test]
fn a_rejected_corrective_does_not_latch_the_mandate() {
    let rig = Rig::new(north_filled());
    rig.gateway
        .fail_on(3, SubmitError::Rejected(String::from("no depth")));
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: the rejected attempt created no economic order, so nothing is \
         covered by it"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 10],
        "FAILED: a rejection creates nothing and leaves the exposure open to a \
         later attempt - a mandate must not be latched shut by one failure"
    );
}

/// Custody inventory aggregates across mandates, runs and dead orders, so it
/// cannot prove that one particular order filled.
#[test]
fn pre_existing_custody_inventory_is_not_a_fill() {
    let rig = Rig::new(vec![batch(vec![], vec![], "k0")]);
    rig.inventory.hold(InstrumentId::new("m1-N"), 30);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert!(
        engine.positions().is_empty(),
        "FAILED: custody inventory held before an order was submitted proves \
         nothing about that order"
    );
    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: no leg filled, so there is no exposure to correct"
    );
    assert_eq!(
        engine.pending_orders().len(),
        2,
        "FAILED: both legs are live and remain tracked"
    );
}

/// Two mandates naming one instrument. Quantity acquired by one must not
/// complete a leg of the other.
#[test]
fn two_mandates_sharing_an_instrument_do_not_cross_attribute() {
    let rig = Rig::with(
        vec![
            candidate_on("m1", "shared-N", "m1-S", 10, at(10_000)),
            candidate_on("m2", "shared-N", "m2-S", 10, at(10_000)),
        ],
        vec![
            batch(vec![], vec![], "k0"),
            // m1's north leg is the first submission: V-101 under pex-1.
            batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        ],
    );
    let mut engine = rig.engine();
    rig.inventory.hold(InstrumentId::new("shared-N"), 10);

    engine.run_cycle().expect("cycle 1");
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    let m2_attempts = rig
        .gateway
        .submitted
        .borrow()
        .iter()
        .filter(|request| request.strategy_id.as_str() == "m2")
        .count();
    assert_eq!(
        m2_attempts, 2,
        "FAILED: mandate m2 sent only the two legs it was entered with - m1's \
         fill on the shared instrument is not m2's"
    );
    assert!(
        rig.store
            .snapshot()
            .positions
            .iter()
            .all(|position| position.strategy_id.as_str() != "m2"),
        "FAILED: quantity acquired by one mandate never completes another"
    );
}

/// The window is tested against the venue's own match timestamp, not against the
/// cycle in which the feed happened to deliver the event.
#[test]
fn the_window_is_tested_against_the_match_time_not_the_cycle() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    // The window runs [OPEN, OPEN + 900). This event is delivered now, but the
    // match it describes happened long after the identifier was recycled.
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c1", 10, at(OPEN + 5_000))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: the attribution window is tested against the venue's match \
         timestamp, not the cycle that read the event"
    );
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c1", 4, at(OPEN + 899))],
        vec![],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: the last instant inside the original window is still inside it, \
         however late the feed delivers the event"
    );
}

/// A restart must not reopen a window: the original window start is what makes
/// `order_id` usable at all, and it is durable state.
#[test]
fn a_restart_does_not_reopen_an_attribution_window() {
    let rig = Rig::new(vec![batch(vec![], vec![], "k0")]);
    {
        let mut engine = rig.engine();
        engine.run_cycle().expect("cycle 1");
    }

    // A restart long after those windows closed, with the feed offering one
    // observation from outside the original window and one from inside it.
    rig.fills.batches.borrow_mut().push(batch(
        vec![
            event("V-101", "pex-1", 10, at(OPEN + 2_000)),
            event("V-102", "pex-2", 10, at(OPEN + 100)),
        ],
        vec![],
        "k1",
    ));
    {
        let mut engine = rig.engine();
        rig.clock.set(at(OPEN + 2_000));
        engine.run_cycle().expect("cycle 2 after restart");
    }

    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        0,
        "FAILED: a restart must not reopen an attribution window - an observation \
         from outside the original one names a recycled identifier"
    );
    assert_eq!(
        attributed_in(&saved, "pex-2"),
        10,
        "FAILED: the original window start survives a restart, so an observation \
         from inside it is still attributed"
    );
    assert!(
        saved.positions.is_empty(),
        "FAILED: the north leg is unconfirmed, so no pairing may be recorded"
    );
    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: an unconfirmed leg is never neutralized against"
    );
}

/// Correcting a resting leg is a sequence: closure of the resting order first,
/// corrective order afterwards.
#[test]
fn the_resting_leg_is_closed_before_the_corrective_is_sent() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    assert!(
        rig.gateway.logged_at("cancel:V-102").is_none(),
        "FAILED: a resting order is not treated as gone merely because a \
         correction has become necessary - the closure is a separate step"
    );

    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    let closed_at = rig.gateway.logged_at("cancel:V-102");
    let corrective_at = rig.gateway.logged_at("submit:V-103");
    assert!(
        closed_at.is_some() && corrective_at.is_some(),
        "FAILED: the cycle that acts closes the resting leg (V-102) and covers \
         what remains (V-103)"
    );
    assert!(
        closed_at.unwrap() < corrective_at.unwrap(),
        "FAILED: a resting leg is moved into closure before a corrective order is \
         issued for whatever remains"
    );
}

/// Both legs seen filled on the live order list: one pairing, at the leg size,
/// and no corrective order.
#[test]
fn a_pairing_is_recorded_at_the_leg_size_when_both_legs_are_seen_filled() {
    let rig = Rig::new(vec![batch(vec![], vec![], "k0")]);
    let mut engine = rig.engine();
    engine.run_cycle().expect("cycle 1");

    rig.gateway.set_open(vec![
        open_order("V-101", "pex-1", 101, 10, 10),
        open_order("V-102", "pex-2", 102, 10, 10),
    ]);
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    let positions = engine.positions();
    assert_eq!(
        positions.len(),
        1,
        "FAILED: filled_qty on the live order list is a fill like any other, so \
         two complete legs record one pairing"
    );
    assert_eq!(
        (positions[0].size_north, positions[0].size_south),
        (10, 10),
        "FAILED: the pairing is recorded at the leg size"
    );
    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: nothing is missing, so nothing is corrected"
    );
    assert!(
        engine.pending_orders().is_empty(),
        "FAILED: a paired mandate stops being tracked"
    );
}

/// An unacknowledged corrective whose fill arrives before any retry: the fill
/// event carries the client reference, so the engine recovers the order's
/// identity, counts the fill against the leg, and never places a second order.
#[test]
fn a_lost_corrective_is_identified_by_its_fill_and_not_resent() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        // The corrective is the third submission; its acknowledgement was lost,
        // but the venue matched it and the feed names its client reference.
        batch(vec![event("V-103", "pex-3", 10, at(OPEN + 180))], vec![], "k3"),
    ]);
    rig.gateway.fail_on(3, SubmitError::AcknowledgementLost);
    let mut engine = rig.engine();

    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    rig.gateway.set_open(vec![]);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: the lost acknowledgement is one attempt at one economic order"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: identity is recovered when a later observation carries the client \
         reference, so the corrective is not submitted a second time"
    );
    let positions = engine.positions();
    assert_eq!(
        positions.len(),
        1,
        "FAILED: a neutralization fill resolves the exposure it was sent for, so \
         the mandate pairs"
    );
    assert_eq!(
        (positions[0].size_north, positions[0].size_south),
        (10, 10),
        "FAILED: quantity attributed to a neutralization is counted once, against \
         the leg it was sent for and not also as a fill of the resting order"
    );
}

/// A failed `open_orders` call says nothing about the venue, so no order is
/// treated as withdrawn and no exposure is covered on the strength of it.
#[test]
fn an_unavailable_open_order_list_concludes_nothing() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);

    engine.run_cycle().expect("cycle 1");
    rig.gateway.open_fails.set(true);
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: a failed open_orders call proves nothing about the venue, so no \
         leg may be treated as withdrawn because of it"
    );
    assert!(
        rig.gateway.cancelled.borrow().is_empty(),
        "FAILED: nothing is cancelled on the strength of a failed read"
    );

    // The venue answers again, and the correction proceeds from there.
    rig.gateway.open_fails.set(false);
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: once the venue answers, the resting leg is closed and the \
         residual covered once"
    );
    assert!(
        rig.gateway.logged_at("cancel:V-102") < rig.gateway.logged_at("submit:V-103"),
        "FAILED: closure of the resting order comes before the corrective order"
    );
}
