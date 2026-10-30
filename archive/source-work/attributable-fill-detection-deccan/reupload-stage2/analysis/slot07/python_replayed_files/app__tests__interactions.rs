//! Scratch checks for rule combinations the single-rule diagnostics cannot see.
//!
//! Not graded, and not part of the two aids. Each case here turns on two or three
//! documented rules interacting.

mod harness;

use harness::*;

use pairing_engine::ports::{FillBatch, SubmitError};
use pairing_engine::types::snapshot::Snapshot;
use pairing_engine::types::Venue;
use pairing_engine::{Engine, EngineConfig, ATTRIBUTION_WINDOW};

const OPEN: i64 = 1_000;

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

fn north_filled() -> Vec<FillBatch> {
    vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
    ]
}

/// Drive to the point where the south leg has been cancelled and one corrective
/// order has been sent for it.
fn to_first_corrective(rig: &Rig, engine: &mut Engine<'_>) {
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    rig.gateway.set_open(vec![]);
}

fn attributed_in(snapshot: &Snapshot, client_ref: &str) -> i64 {
    snapshot
        .submissions
        .iter()
        .find(|record| record.client_ref == client(client_ref))
        .map(|record| record.attributed())
        .expect("every order the engine submits is tracked")
}

/// A lost acknowledgement has to survive a restart, and the retry after the
/// restart has to go out under the same client reference.
#[test]
fn a_lost_acknowledgement_survives_a_restart() {
    let rig = Rig::new(north_filled());
    rig.gateway.fail_on(3, SubmitError::AcknowledgementLost);

    {
        let mut engine = rig.engine();
        to_first_corrective(&rig, &mut engine);
    }
    let saved = rig.store.snapshot();
    let lost = &saved.neutralizations[0];
    assert!(lost.order_id.is_none(), "the record keeps no venue id");
    assert_eq!(lost.client_ref.as_str(), "pex-3");

    // Restart, and let the retry land.
    {
        let mut engine = rig.engine();
        rig.clock.set(at(OPEN + 180));
        engine.run_cycle().expect("cycle 4 after restart");
    }

    let refs = rig.gateway.corrective_refs();
    assert_eq!(refs.len(), 2, "one original attempt and one retry");
    assert_eq!(
        refs[1], refs[0],
        "the retry after a restart uses the client reference the venue already knows"
    );
}

/// A corrective that fills slowly must not draw a second order, and must stop
/// being counted as working once the venue closes it.
#[test]
fn a_slowly_filling_corrective_is_not_covered_twice() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    to_first_corrective(&rig, &mut engine);
    assert_eq!(rig.gateway.corrective_sizes(), vec![10]);

    // The corrective reports 4 of 10, and stays live.
    rig.gateway
        .set_open(vec![open_order("V-103", "pex-3", 103, 10, 4)]);
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "4 units are working, so the residual is zero"
    );

    // The venue closes it in a block that leaves 6 units unfilled.
    rig.gateway.set_open(vec![]);
    rig.fills.batches.borrow_mut().push(batch(
        vec![],
        vec![block("bZ", &["V-103"], 1, at(OPEN + 185))],
        "k9",
    ));
    rig.clock.set(at(OPEN + 240));
    engine.run_cycle().expect("cycle 5");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 5],
        "a closed corrective works nothing more, so the 5 units still missing are \
         covered - and only those 5"
    );
}

/// Two mandates on one instrument, netted into one block: each takes its own
/// share and neither completes the other.
#[test]
fn a_shared_block_divides_between_two_mandates() {
    let mut ledger = AttributionLedger::new();
    // Mandate A's north leg, and mandate B's north leg: same instrument, one
    // block, sorted by window start then venue sequence.
    ledger.track(client("a1"), Some(order("oA")), Some(7), 10, at(OPEN));
    ledger.track(client("b1"), Some(order("oB")), Some(8), 30, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["oB", "oA"], 20, at(OPEN + 60))],
        "k1",
    ));
    // Basis 40, q 20: 20*10/40 = 5 and 20*30/40 = 15, remainder 0.
    assert_eq!(ledger.attributed_for(&client("a1")), 5);
    assert_eq!(ledger.attributed_for(&client("b1")), 15);
}

/// A block at exactly the window end allocates nothing, and still closes the
/// order it names.
#[test]
fn a_block_at_the_window_end_allocates_nothing_but_still_closes() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    let _ = ledger.absorb_batch(batch(
        vec![],
        vec![block("b1", &["o1"], 10, at(OPEN) + ATTRIBUTION_WINDOW)],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "an instant exactly at the window end is outside the half-open window"
    );
    assert_eq!(
        ledger.outstanding_for(&client("c1")),
        0,
        "the block still closes the order it names, so nothing is still working"
    );
}

/// An observation whose client reference the engine holds is attributed to that
/// reference, not to whichever record happens to carry the venue id — the venue
/// recycles ids once a window has closed.
#[test]
fn the_client_reference_wins_over_a_recycled_venue_id() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    ledger.track(client("c2"), None, None, 10, at(OPEN + 2_000));
    // o1's window has long closed; the venue has handed the id on, and this
    // event names c2 while carrying it.
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c2", 10, at(OPEN + 2_100))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "an observation outside c1's window says nothing about c1"
    );
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        10,
        "the engine's own key resolves the record, and the identity is recovered"
    );
}

/// A restart mid-closure: the closure state, the attributed quantity and the
/// feed position all survive, and the corrective is sent once, not twice.
#[test]
fn a_restart_mid_closure_does_not_send_the_correction_twice() {
    let rig = Rig::new(north_filled());
    {
        let mut engine = rig.engine();
        rig.gateway
            .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
        engine.run_cycle().expect("cycle 1");
        rig.clock.set(at(OPEN + 60));
        engine.run_cycle().expect("cycle 2 marks the south leg");
        assert!(rig.gateway.corrective_sizes().is_empty());
    }

    // Restart before the cancel, with the feed replaying the north fill.
    rig.fills
        .batches
        .borrow_mut()
        .push(batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"));
    {
        let mut engine = rig.engine();
        rig.clock.set(at(OPEN + 120));
        engine.run_cycle().expect("cycle 3 after restart: cancel and cover");
        rig.clock.set(at(OPEN + 180));
        engine.run_cycle().expect("cycle 4");
    }

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "one corrective order, sized to the residual, across a restart"
    );
    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        10,
        "the replayed fill does not add to what was already attributed"
    );
}

/// A leg that fills in the cycle after it was cancelled is still attributed, and
/// the corrective already committed does not cover the same units again.
#[test]
fn an_in_flight_fill_is_attributed_and_shrinks_the_gap() {
    let rig = Rig::new(north_filled());
    let mut engine = rig.engine();
    to_first_corrective(&rig, &mut engine);
    assert_eq!(rig.gateway.corrective_sizes(), vec![10]);

    // The cancelled south leg had 6 units matched before the cancel landed.
    rig.fills.batches.borrow_mut().push(batch(
        vec![event("V-102", "pex-2", 6, at(OPEN + 130))],
        vec![],
        "k9",
    ));
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "10 units are already working on the corrective, so nothing more is sent"
    );
    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-2"),
        6,
        "the fill that was in flight when the leg was cancelled still counts"
    );
}

/// An entry submission whose acknowledgement was lost is recovered when a later
/// observation carries the client reference back.
#[test]
fn a_lost_entry_is_recovered_from_a_later_observation() {
    let rig = Rig::new(vec![batch(vec![], vec![], "k0")]);
    rig.gateway.fail_on(1, SubmitError::AcknowledgementLost);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1: the north leg is lost");
    assert_eq!(engine.pending_orders().len(), 1, "only the south leg is tracked");

    // The venue did create it. It shows up on the live list under the reference
    // the engine sent it under.
    rig.gateway.set_open(vec![open_order(
        "V-101",
        "pex-1",
        101,
        10,
        0,
    )]);
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    let legs = engine.pending_orders();
    assert_eq!(legs.len(), 2, "the lost leg is recovered");
    let north: Vec<_> = legs.iter().filter(|leg| leg.venue == Venue::North).collect();
    assert_eq!(north.len(), 1);
    assert_eq!(north[0].client_ref.as_str(), "pex-1");
    assert_eq!(
        rig.gateway
            .submitted
            .borrow()
            .iter()
            .filter(|request| request.client_ref.as_str() == "pex-1")
            .count(),
        2,
        "the recovery reuses the reference, so no second economic order is placed"
    );
}
