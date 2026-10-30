//! Scratch checks for combinations of rules the single-rule diagnostics cannot
//! see. Not graded; scratch working area.

mod harness;

use harness::*;

use pairing_engine::engine::AttributionLedger;
use pairing_engine::ports::{FillBatch, SubmitError};
use pairing_engine::types::snapshot::Snapshot;
use pairing_engine::{Engine, EngineConfig};

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

    fn both_resting(&self) {
        self.gateway.set_open(vec![
            open_order("V-101", "pex-1", 101, 10, 0),
            open_order("V-102", "pex-2", 102, 10, 0),
        ]);
    }
}


fn attributed_in(snapshot: &Snapshot, client_ref: &str) -> i64 {
    snapshot
        .submissions
        .iter()
        .find(|record| record.client_ref == client(client_ref))
        .map(|record| record.attributed())
        .unwrap_or(-1)
}

fn finalized_in(snapshot: &Snapshot, client_ref: &str) -> bool {
    snapshot
        .submissions
        .iter()
        .find(|record| record.client_ref == client(client_ref))
        .map(|record| record.finalized)
        .unwrap_or(false)
}

/// A block closes the unfilled leg. The venue cancelled it, so it is not
/// unconfirmed: the residual it leaves behind is a real exposure and must be
/// covered. The block also closes a covered order it allocated nothing to.
#[test]
fn a_block_finalized_leg_is_neutralized_for_its_residual() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-101", "V-102"], 5, at(OPEN + 30))],
            "k1",
        ),
    ]);
    // North has already filled 10 of 10 when bA is divided, so north has no
    // outstanding quantity and is not eligible. bA allocates its whole 5 to the
    // south leg and closes both orders.
    rig.both_resting();
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    let saved = rig.store.snapshot();
    assert_eq!(attributed_in(&saved, "pex-1"), 10, "north leg fills in full");
    assert_eq!(attributed_in(&saved, "pex-2"), 5, "south takes the whole block");
    assert!(
        finalized_in(&saved, "pex-1"),
        "the block allocated north nothing, and still closed it"
    );
    assert!(finalized_in(&saved, "pex-2"), "the block closes the south leg");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![5],
        "a leg the venue closed is not unconfirmed: the 5 units the block did \
         not fill are a real exposure and draw one corrective order"
    );
}

/// A block that closes a corrective order leaves a real gap behind. The mandate
/// must be able to make progress again rather than stay latched at zero
/// residual.
#[test]
fn a_finalized_corrective_order_leaves_the_gap_open() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-102"], 3, at(OPEN + 30))],
            "k1",
        ),
        batch(vec![], vec![block("bB", &["V-103"], 2, at(OPEN + 90))], "k2"),
        batch(vec![], vec![], "k3"),
    ]);
    rig.both_resting();
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![7],
        "the first corrective is sized to the residual the block left"
    );

    // bB closes the corrective after it filled 2 of 7: 5 units are still short.
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![7, 5],
        "a corrective the venue closed contributes no working quantity, so the \
         5 units it never filled are covered by a second corrective"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![7, 5],
        "the second corrective is already working, so no third is sent"
    );
}

/// A rejection creates nothing. The mandate must not be latched shut: a later
/// cycle recomputes the residual and covers it.
#[test]
fn a_rejected_corrective_order_is_retried_on_a_later_cycle() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        batch(vec![], vec![], "k3"),
    ]);
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    rig.gateway
        .fail_on(3, SubmitError::Rejected("no liquidity".into()));
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "the rejected attempt still reached the venue as a request"
    );

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 10],
        "a rejection creates no economic order, so the exposure stays open and \
         a later cycle covers it"
    );
}

/// The venue recycles order identifiers once a window has closed. An
/// observation naming a recycled identifier is not evidence about the order
/// this engine tracked.
#[test]
fn an_observation_outside_the_window_names_another_order() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 6, at(OPEN))], vec![], "k1"),
        batch(vec![event("V-101", "pex-1", 9, at(OPEN + 5_000))], vec![], "k2"),
    ]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 30));
    engine.run_cycle().expect("cycle 2");
    // Long afterwards the feed replays the identifier for an unrelated order.
    rig.clock.set(at(OPEN + 6_000));
    engine.run_cycle().expect("cycle 3");

    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        6,
        "the event after the window closed names a recycled identifier and is \
         not attributed to this order"
    );
}

/// A block whose covered orders are all outside their windows allocates
/// nothing, and still closes every tracked order it names.
#[test]
fn a_block_outside_every_window_allocates_nothing_but_finalizes() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-102"], 9, at(OPEN + 5_000))],
            "k1",
        ),
    ]);
    rig.both_resting();
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-2"),
        0,
        "a block whose match time is outside every window allocates nothing"
    );
    assert!(
        finalized_in(&saved, "pex-2"),
        "the venue closes every order a block names, even one it allocated nothing to"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "the whole leg is still short once the out-of-window block is ignored"
    );
}

/// A leg's own partial fill reduces what the corrective order has to cover.
#[test]
fn a_partial_fill_reduces_the_corrective_order() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-102"], 6, at(OPEN + 30))],
            "k1",
        ),
    ]);
    rig.both_resting();
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![4],
        "the corrective covers the required quantity less what the leg already \
         acquired"
    );
}

/// Two mandates on one instrument: a block naming one mandate's order says
/// nothing about the other's.
#[test]
fn a_block_does_not_cross_mandates() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![], vec![block("bA", &["V-101"], 5, at(OPEN + 30))], "k1"),
    ]);
    rig.strategies
        .candidates
        .borrow_mut()
        .push(candidate("m2", 10, at(10_000)));
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        5,
        "m1's north leg takes the block"
    );
    assert_eq!(
        attributed_in(&saved, "pex-3"),
        0,
        "m2's north leg takes nothing from a block that does not name it"
    );
    assert!(!finalized_in(&saved, "pex-3"));
}

/// A restart must not reopen an attribution window: the original window start
/// is durable, and an observation after the restart that falls outside the
/// original window is still outside it.
#[test]
fn a_restart_does_not_reopen_an_attribution_window() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 6, at(OPEN))], vec![], "k1"),
    ]);
    {
        let mut engine = rig.engine();
        engine.run_cycle().expect("cycle 1");
        rig.clock.set(at(OPEN + 30));
        engine.run_cycle().expect("cycle 2");
    }

    // Restart long afterwards, with the feed replaying an event timestamped
    // outside the original window.
    rig.clock.set(at(OPEN + 5_000));
    rig.fills
        .batches
        .borrow_mut()
        .push(batch(vec![event("V-101", "pex-1", 9, at(OPEN + 4_000))], vec![], "k2"));
    {
        let mut engine = rig.engine();
        engine.run_cycle().expect("cycle after restart");
    }

    let saved = rig.store.snapshot();
    let record = saved
        .submissions
        .iter()
        .find(|record| record.client_ref == client("pex-1"))
        .expect("the submission survives the restart");
    assert_eq!(
        record.window_start,
        at(OPEN),
        "the original window start is durable state; a restart must not reopen it"
    );
    assert_eq!(
        record.attributed(),
        6,
        "an observation outside the original window is still outside it after a \
         restart"
    );
}

/// Quantity attributed to a corrective order counts toward the leg it was sent
/// for, and only toward that leg.
#[test]
fn a_corrective_fill_counts_once_toward_its_own_leg() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        batch(
            vec![event("V-103", "pex-3", 10, at(OPEN + 150))],
            vec![],
            "k3",
        ),
    ]);
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    assert_eq!(rig.gateway.corrective_sizes(), vec![10]);

    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");
    rig.clock.set(at(OPEN + 240));
    engine.run_cycle().expect("cycle 5");

    assert!(
        engine
            .positions()
            .iter()
            .any(|p| p.strategy_id.as_str() == "m1"),
        "once the corrective order fills, the leg it was sent for is complete \
         and the pairing is recorded"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "the corrective order is not sent twice"
    );
}

/// An order whose acknowledgement was lost may exist at the venue. A fill
/// reported under its client reference is attributed to it, and the retry
/// resolves onto the same order rather than placing a second one.
#[test]
fn a_lost_acknowledgement_recovers_identity_from_a_later_observation() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        batch(
            vec![event("V-103", "pex-3", 10, at(OPEN + 150))],
            vec![],
            "k3",
        ),
        batch(vec![], vec![], "k4"),
    ]);
    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    rig.gateway.fail_on(3, SubmitError::AcknowledgementLost);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 120));
    engine.run_cycle().expect("cycle 3");
    // The venue did create it, and reports the fill under the client reference.
    rig.clock.set(at(OPEN + 180));
    engine.run_cycle().expect("cycle 4");

    assert!(
        engine
            .positions()
            .iter()
            .any(|p| p.strategy_id.as_str() == "m1"),
        "a fill reported under the retained client reference resolves the lost \
         acknowledgement and completes the leg"
    );
    assert_eq!(
        rig.gateway.corrective_refs(),
        vec!["pex-3"],
        "the fill proved the order existed, so no second economic order is sent"
    );
}

/// `filled_qty` on the live order list and `cumulative_filled_qty` on a fill
/// event are the same quantity seen from two sides, not two separate fills.
#[test]
fn the_live_list_and_the_feed_describe_one_quantity() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![event("V-101", "pex-1", 7, at(OPEN + 10))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
    ]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    // The venue lists the order as live with the same running total, and the
    // feed delivers the same event again.
    rig.gateway.set_open(vec![open_order("V-101", "pex-1", 101, 10, 7)]);
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        7,
        "the live list and the feed report one quantity; neither is added to \
         the other"
    );
}

/// The venue recycles an identifier once the first order's window has closed.
/// An observation naming the recycled identifier belongs to the later order,
/// and the client reference is what says so.
#[test]
fn a_recycled_identifier_belongs_to_the_later_order() {
    let mut ledger = AttributionLedger::new();
    ledger.track(client("c1"), Some(order("o1")), Some(1), 10, at(OPEN));
    // The identifier o1 is handed out again long after c1's window closed.
    ledger.track(client("c2"), Some(order("o1")), Some(2), 10, at(OPEN + 5_000));
    let _ = ledger.absorb_batch(batch(
        vec![event("o1", "c2", 6, at(OPEN + 5_100))],
        vec![],
        "k1",
    ));
    assert_eq!(
        ledger.attributed_for(&client("c2")),
        6,
        "an observation naming a recycled identifier is attributed to the order \
         whose client reference it carries"
    );
    assert_eq!(
        ledger.attributed_for(&client("c1")),
        0,
        "the first holder of the identifier acquires nothing from an observation \
         about another order"
    );
}

/// The window is tested against the venue's match timestamp, not against the
/// cycle that happened to read the event: the feed may lag for many cycles.
#[test]
fn a_late_event_timestamped_inside_the_window_is_attributed() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(vec![], vec![], "k1"),
        batch(vec![event("V-101", "pex-1", 8, at(OPEN + 60))], vec![], "k2"),
    ]);
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    // Several cycles later the feed delivers an event whose match time is well
    // inside the window this order opened in cycle 1.
    rig.clock.set(at(OPEN + 700));
    engine.run_cycle().expect("cycle 2");
    rig.clock.set(at(OPEN + 800));
    engine.run_cycle().expect("cycle 3");

    let saved = rig.store.snapshot();
    assert_eq!(
        attributed_in(&saved, "pex-1"),
        8,
        "the window is tested against the venue's match timestamp, so a late \
         event about an in-window match is still attributed"
    );
}

/// Quantity above the basis belongs to orders this engine does not track, so it
/// is dropped rather than assigned.
#[test]
fn a_block_larger_than_the_basis_drops_the_excess() {
    let rig = Rig::new(vec![
        batch(vec![], vec![], "k0"),
        batch(
            vec![],
            vec![block("bA", &["V-101", "V-102"], 25, at(OPEN + 30))],
            "k1",
        ),
    ]);
    rig.both_resting();
    let mut engine = rig.engine();

    engine.run_cycle().expect("cycle 1");
    rig.clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    let saved = rig.store.snapshot();
    let total: i64 = saved
        .submissions
        .iter()
        .filter(|record| record.client_ref == client("pex-1") || record.client_ref == client("pex-2"))
        .map(|record| record.attributed())
        .sum();
    assert_eq!(
        total, 20,
        "the block matched 25 against 20 outstanding; the 5 units no tracked \
         order can account for are dropped"
    );
}
