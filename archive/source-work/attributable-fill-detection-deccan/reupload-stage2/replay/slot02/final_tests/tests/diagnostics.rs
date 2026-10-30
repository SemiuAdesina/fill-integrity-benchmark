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
use pairing_engine::{Engine, EngineConfig, Qty, ATTRIBUTION_WINDOW};

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

/// The venue builds `covers` in whatever order it likes, and the outcome must
/// not depend on that: only the documented ordering decides it.
#[test]
fn the_order_of_a_blocks_covers_list_decides_nothing() {
    let allocations = |covers: Vec<&str>| {
        let mut ledger = AttributionLedger::new();
        ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
        ledger.track(client("c2"), Some(order("o2")), Some(2), 10, at(OPEN));
        ledger.track(client("c3"), Some(order("o3")), Some(3), 30, at(OPEN));
        let _ = ledger.absorb_batch(batch(
            vec![],
            vec![block("b1", &covers, 37, at(OPEN + 60))],
            "k1",
        ));
        ["c1", "c2", "c3"]
            .iter()
            .map(|reference| ledger.attributed_for(&client(reference)))
            .collect::<Vec<Qty>>()
    };

    let forward = allocations(vec!["o1", "o2", "o3"]);
    for permutation in [
        vec!["o1", "o3", "o2"],
        vec!["o2", "o1", "o3"],
        vec!["o2", "o3", "o1"],
        vec!["o3", "o1", "o2"],
        vec!["o3", "o2", "o1"],
    ] {
        assert_eq!(
            allocations(permutation),
            forward,
            "FAILED: the iteration order of the container holding the orders must \
             never decide an attribution outcome"
        );
    }
    assert_eq!(
        forward,
        vec![8, 7, 22],
        "FAILED: a block total is divided by the documented ordering, one \
         remainder unit to each of the first r orders of it"
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

/// The basis an order is divided against is its *outstanding* quantity — size
/// less what is already attributed — not the size it was submitted for.
#[test]
fn a_block_divides_against_outstanding_quantity_not_original_size() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    // 6 of c1's 10 base units have already arrived directly, so only 4 of it is
    // still outstanding when the block is matched.
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c1", 6, at(OPEN))],
        vec![],
        "k1",
    ));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 17, at(OPEN + 60))],
        "k2",
    ));
    // basis is 4 + 30 = 34, so the shares are 17*4/34 = 2 and 17*30/34 = 15.
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: the basis is the sum of the eligible set's outstanding quantity, \
         not of the orders' original size - 17*4/34 is 2, not 5"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        15,
        "FAILED: the basis is the sum of the eligible set's outstanding quantity, \
         not of the orders' original size - 17*30/34 is 15"
    );
}

/// A block closes every order it names, so one the venue has already closed
/// contributes no outstanding quantity to a later block's basis and acquires
/// nothing further, whatever it was short of.
#[test]
fn a_block_finalizes_every_order_it_covers() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o2")), Some(2), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1", "o2"], 30, at(OPEN + 60))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: a block share is (q * outstanding) / basis plus at most one \
         remainder unit"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        22,
        "FAILED: a block share is (q * outstanding) / basis plus at most one \
         remainder unit"
    );

    // A later block naming the same orders: the venue closed them, so they are
    // no longer outstanding and nothing further is allocated to them.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o2", "o1"], 5, at(OPEN + 120))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        8,
        "FAILED: a finalized order contributes no outstanding quantity to a later \
         block's basis - the venue closed it and cancelled the rest"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        22,
        "FAILED: a finalized order contributes no outstanding quantity to a later \
         block's basis - the venue closed it and cancelled the rest"
    );
}

/// A block whose eligible set is empty allocates nothing, and still closes every
/// tracked order it names.
#[test]
fn a_block_with_no_eligible_orders_allocates_nothing() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    // The block's match timestamp falls outside the only covered order's window.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bA", &["o1"], 9, window_end())],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: a block whose covered orders are all outside their windows is \
         evidence about some other order, not about these"
    );
    // Finalized all the same, so a later block inside its window gives it nothing.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("bB", &["o1"], 9, at(OPEN + 60))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: a block finalizes every tracked order it covers, including one \
         the division skipped"
    );
}

/// An order whose acknowledgement was lost has no venue id until a later
/// observation carries one back under the engine's own `client_ref`.
#[test]
fn an_unacknowledged_order_is_found_by_its_client_ref() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), None, None, 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![event("o-late", "c1", 6, at(OPEN + 5))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        6,
        "FAILED: client_ref is the key the engine always knows, so an observation \
         carrying it reaches an order the venue never acknowledged"
    );
    assert_eq!(
        ledger.order_for(&client("c1")),
        Some(order("o-late")),
        "FAILED: identity is recovered when a later observation carries the \
         client_ref"
    );
}

/// The venue recycles identifiers once a window has closed, so an observation
/// naming a recycled id is attributed to the order whose own window contains its
/// match timestamp — and to nothing at all when no tracked order's window does.
#[test]
fn an_observation_outside_every_window_is_attributed_to_nothing() {
    let mut ledger = AttributionLedger::new();
    // The same venue id, two unrelated orders: the older window has closed.
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), Some(order("o1")), Some(9), 10, at(OPEN + 5_000));
    // A replay of the old order's fill: inside c1's window, outside c2's.
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c1", 7, at(OPEN + 10))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        7,
        "FAILED: a fill is attributed to the order whose window contains the \
         venue's match timestamp"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        0,
        "FAILED: an observation timestamped outside an order's window names an \
         identifier that is no longer that order, and counting it is a \
         misattribution"
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

/// An observation outside an order's window names an identifier that is no longer
/// that order. Its quantity is dropped, and so is the label: binding the label
/// would let a later block naming the recycled id close this order, or allocate
/// it base units, on the strength of a replay.
#[test]
fn an_out_of_window_observation_does_not_bind_the_venues_label() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), None, None, 10, at(OPEN));
    // A replay long after the window closed: the id now belongs to another order.
    let _ = ledger.absorb_batch(batch(
        vec![event("recycled", "c1", 6, at(OPEN + 5_000))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: an observation outside an order's window is attributed no quantity"
    );
    assert_eq!(
        ledger.order_for(&client("c1")),
        None,
        "FAILED: an observation outside an order's window does not teach the ledger \
         that order's venue id - the label it carries names a different order"
    );

    // The id has been handed to someone else; a block naming it is about them.
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["recycled"], 10, at(OPEN + 5_100))],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "FAILED: a block naming a label this order never owned allocates it nothing"
    );
    assert!(
        !ledger.is_finalized(&client("c1")),
        "FAILED: a block does not close an order whose label it does not carry - \
         the venue closed a different order"
    );
    assert_eq!(
        ledger.outstanding_for(&client("c1")),
        10,
        "FAILED: an order no block has named is still open for its whole size"
    );
}

/// A block is final for every order it covers: the venue has closed the order and
/// cancelled whatever the block did not fill, so it never acquires another base
/// unit. A reading the feed replays afterwards moves nothing.
#[test]
fn a_finalized_order_never_acquires_another_base_unit() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1"], 4, at(OPEN + 10))],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: a block allocates its divisible quantity to the orders it covers"
    );

    // A replay of a fill the block superseded: in-window, but too late.
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c1", 10, at(OPEN + 11))],
        vec![],
        "k2",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: a finalized order will never acquire another base unit, so a \
         replayed cumulative reading cannot raise what the block left"
    );

    // Nor can the live order list, which does not carry a covered order at all.
    ledger.absorb_open_orders(&[open_order("o1", "c1", 1, 10, 9)]);
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        4,
        "FAILED: an order the venue has closed is not live, so no open-order entry \
         can add quantity to it"
    );
    assert!(
        ledger.is_finalized(&client("c1")),
        "FAILED: a block finalizes every order it names"
    );
}

/// A fill the venue genuinely reported before its own block still counts: the
/// ledger folds events before blocks, so finalization cannot swallow it.
#[test]
fn a_fill_reported_before_the_block_that_closed_it_still_counts() {
    let mut ledger = ledger_with("o1", "c1", 1, 10);
    let _ = ledger.absorb_batch(
        batch(
            vec![event("o1", "c1", 6, at(OPEN + 5))],
            vec![block("b1", &["o1"], 4, at(OPEN + 10))],
            "k1",
        ),
    );
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        10,
        "FAILED: a block divides over what its covered orders still had outstanding, \
         so a fill reported before the block is kept and the block supplies the rest"
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

/// A failed `open_orders` says nothing about the venue, so no order can be
/// assumed live — and none can be assumed gone either.
#[test]
fn a_failed_open_orders_call_proves_nothing() {
    let rig = Rig::new(north_filled());
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    // The venue's live-order list is unavailable on the cycle the north leg fills.
    rig.gateway.open_unavailable.set(true);
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: a leg whose state is unknown is never neutralized against - a \
         failed open_orders call says nothing about the venue state"
    );
    assert!(
        rig.gateway.cancelled.borrow().is_empty(),
        "FAILED: a failed open_orders call does not show an order as resting, so \
         there is nothing to move into closure"
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

/// Correcting a resting leg is a sequence, not one step: the resting order is
/// moved into closure first, and the corrective order follows in a later cycle,
/// so that a fill already in flight has a cycle in which to land.
#[test]
fn a_resting_leg_is_closed_before_a_corrective_order_is_sent() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    rig.gateway.set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        Vec::<Qty>::new(),
        "FAILED: a resting order is not treated as gone merely because a \
         correction has become necessary - the cycle that closes it sends no \
         corrective order"
    );
    assert_eq!(
        rig.gateway.cancelled.borrow().len(),
        1,
        "FAILED: the first cycle that finds the exposure moves the resting order \
         into closure at the venue"
    );

    // The cancellation boundary has now passed, and the venue no longer lists it.
    rig.gateway.set_open(vec![]);
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: only once the cancellation boundary has passed may the \
         corrective order be issued, for whatever residual then remains"
    );
}

/// A rejection creates nothing, so it is not a commitment: the exposure stays
/// open and a later cycle recomputes the residual and tries again.
#[test]
fn a_rejected_corrective_order_leaves_the_exposure_open_to_a_later_attempt() {
    let rig = Rig::new(north_filled());
    rig.gateway.fail_on(3, SubmitError::Rejected("no depth".into()));
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);

    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: a rejected submission creates nothing, so no order was sent for \
         the exposure"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: a mandate is not latched shut by one failed attempt - a later \
         cycle recomputes the residual and tries again"
    );
}

/// A leg the venue itself closed - a block named it - is not still working, so
/// what it is short of is a residual to cover rather than an unknown.
#[test]
fn a_leg_the_venue_closed_is_covered_for_the_quantity_it_is_short() {
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

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![6],
        "FAILED: a block closes the order and cancels whatever it did not fill, \
         so the gap it leaves behind is real and is the residual to cover"
    );

    // And once that gap is working on a corrective order, it is not covered twice.
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![6],
        "FAILED: quantity still working on a committed neutralization is not \
         exposure"
    );
}

/// A retry that the venue rejects creates nothing, so it stops being a
/// commitment: the exposure is recomputed and a later attempt covers it.
#[test]
fn a_rejected_retry_leaves_the_exposure_open_to_be_recomputed() {
    let rig = Rig::new(north_filled());
    rig.gateway.fail_on(3, SubmitError::AcknowledgementLost);
    rig.gateway.fail_on(4, SubmitError::Rejected("gone".into()));
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    let refs = rig.gateway.corrective_refs();
    assert_eq!(
        refs.len(),
        3,
        "FAILED: a rejected retry creates nothing, so the exposure is still open \
         and a later cycle recomputes the residual and tries again"
    );
    assert_eq!(
        refs[1], refs[0],
        "FAILED: a retry of a lost acknowledgement reuses the client_ref, which is \
         the venue's idempotency key"
    );
    assert_ne!(
        refs[2], refs[0],
        "FAILED: a rejected reference created no order to deduplicate onto, so the \
         recomputed attempt is a fresh one"
    );
    assert_eq!(
        rig.store.snapshot().neutralizations.len(),
        1,
        "FAILED: a rejected retry is not a commitment - the engine's remaining \
         commitment is the recomputed order it just sent"
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

/// A restart must not reopen an attribution window. The window the venue granted
/// runs from the instant the order was submitted, and a restart that restarts it
/// admits an observation from outside the original one.
#[test]
fn a_restart_does_not_reopen_an_attribution_window() {
    // The north leg fills 4 of its 10 before the restart.
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 4, at(OPEN))], vec![], "k1"),
    ]);
    {
        let mut engine = rig.engine();
        engine.run_cycle().expect("cycle 1");
        rig.clock.set(at(OPEN + 60));
        engine.run_cycle().expect("cycle 2");
    }
    let saved = rig.store.snapshot();
    let window_start = saved
        .submissions
        .iter()
        .find(|record| record.client_ref == client("pex-1"))
        .map(|record| record.window_start)
        .expect("FAILED: every order the engine submits is tracked in the snapshot");
    assert_eq!(
        window_start,
        at(OPEN),
        "FAILED: each tracked order's original attribution window start is \
         durable state - a restart must not move it"
    );

    // Long after the original window has closed the feed replays an event under
    // the same venue id. The venue recycles identifiers, so this names some
    // other order now.
    rig.fills.batches.borrow_mut().push(batch(
        vec![event("V-101", "pex-1", 10, at(OPEN + 5_000))],
        vec![],
        "k2",
    ));
    {
        let mut engine = rig.engine();
        rig.clock.set(at(OPEN + 5_000));
        engine.run_cycle().expect("cycle 3 after restart");
    }

    assert_eq!(
        attributed_in(&rig.store.snapshot(), "pex-1"),
        4,
        "FAILED: an observation timestamped outside an order's window names an \
         identifier that is no longer that order, even after a restart - the 6 \
         further units are not this engine's order's"
    );
}

/// Quantity already committed to a correction is still working after a restart,
/// so the restart does not cover the same exposure a second time.
#[test]
fn a_committed_neutralization_survives_a_restart() {
    let rig = Rig::new(north_filled());
    {
        let mut engine = rig.engine();
        engine_after_first_corrective(&rig, &mut engine);
    }
    let saved = rig.store.snapshot();
    assert_eq!(
        saved.neutralizations.len(),
        1,
        "FAILED: committed neutralizations are durable state, including the \
         client_ref a retry depends on"
    );

    let mut engine = rig.engine();
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4 after restart");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: quantity still working in a committed neutralization is not \
         exposure, before or after a restart"
    );
}

/// A neutralization the venue closed part-filled leaves a real gap, and a later
/// cycle covers the remainder rather than the whole exposure again.
#[test]
fn a_finalized_neutralization_leaves_its_gap_to_be_covered_again() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    engine_after_first_corrective(&rig, &mut engine);
    assert_eq!(rig.gateway.corrective_sizes(), vec![10]);

    // The corrective order filled 4 of its 10, and then the venue closed it:
    // a block naming it, allocating the rest nothing.
    rig.fills.batches.borrow_mut().push(batch(
        vec![event("V-103", "pex-3", 4, at(OPEN + 120))],
        vec![block("bA", &["V-103"], 0, at(OPEN + 120))],
        "k2",
    ));
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 6],
        "FAILED: a finalized neutralization contributes nothing as working, \
         whatever it managed to fill, so the residual is the 6 still short"
    );
}
