//! Whole-cycle checks for the combinations the single-rule diagnostics cannot
//! see.
//!
//! `tests/diagnostics.rs` checks one documented rule at a time on synthetic
//! values. The graded situations turn on two or three rules interacting, so this
//! file drives complete cycles through `Engine::run_cycle` and the injected
//! ports and asserts on observable outcomes: which pairings are recorded, which
//! corrective orders are sent, at what size, and in what order relative to the
//! cancellations that precede them.
//!
//! Like the diagnostics, this file is a self-check and is not graded.

mod harness;

use harness::*;

use pairing_engine::ports::{FillBatch, OrderKind, SubmitError};
use pairing_engine::types::ids::{InstrumentId, StrategyId};
use pairing_engine::types::strategy::StrategyCandidate;
use pairing_engine::types::time::Timestamp;
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
    /// One mandate of `depth` a side, expiring far enough out that nothing is
    /// retired while a check runs.
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

    /// Run one cycle at `seconds`, advancing the clock first.
    fn cycle(&self, engine: &mut Engine<'_>, seconds: i64) {
        self.clock.set(at(seconds));
        engine.run_cycle().expect("cycle");
    }
}

/// One empty batch so the first cycle starts the feed, then the interesting one.
fn lead_in() -> FillBatch {
    batch(vec![], vec![], "k0")
}

// ---------------------------------------------------------------------------

/// A block closes a resting leg and cancels whatever it did not fill. The
/// leg is therefore not "momentarily unknown" — the venue has closed it — so the
/// real gap it leaves behind has to be covered.
#[test]
fn a_block_closed_leg_is_corrected_for_the_gap_it_leaves() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(
            vec![event("V-101", "pex-1", 10, at(OPEN))],
            vec![block("bA", &["V-102"], 4, at(OPEN))],
            "k1",
        ),
    ]);
    let mut engine = rig.engine();

    rig.cycle(&mut engine, OPEN);
    rig.cycle(&mut engine, OPEN + 60);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![6],
        "FAILED: a block finalizes every order it covers, so a leg the block \
         closed and only part-filled is covered for the gap it leaves - 10 \
         required, 4 attributed, 6 to neutralize"
    );
    assert!(
        rig.gateway.cancelled.borrow().is_empty(),
        "FAILED: a leg the venue has already closed is not something to cancel"
    );
}

/// A rejected attempt creates nothing, so the exposure stays open and a later
/// cycle recomputes it. A mandate is not latched shut by one failed attempt.
#[test]
fn a_rejected_corrective_leaves_the_exposure_open_to_a_later_attempt() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
    ]);
    rig.gateway.fail_on(3, SubmitError::Rejected("nope".into()));
    let mut engine = rig.engine();

    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    rig.cycle(&mut engine, OPEN);
    rig.cycle(&mut engine, OPEN + 60);
    rig.cycle(&mut engine, OPEN + 120);
    assert_eq!(
        rig.gateway.corrective_refs(),
        vec!["pex-3"],
        "FAILED: one attempt has been made and rejected"
    );

    rig.cycle(&mut engine, OPEN + 180);

    // The rejected attempt created nothing, so the exposure is still open and is
    // recomputed. Nothing exists under the old reference, so the retry is free
    // to use a fresh one: it is not resubmitting an order the venue knows.
    assert_eq!(
        rig.gateway.corrective_refs(),
        vec!["pex-3", "pex-4"],
        "FAILED: a rejection creates nothing and leaves the exposure open - a \
         later cycle recomputes the residual and tries again rather than \
         latching the mandate shut"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 10],
        "FAILED: the residual is unchanged by a rejected attempt, so the later \
         attempt is for the same exposure, not for a second one"
    );
}

/// A corrective the venue finalized contributed nothing still working, whatever
/// it managed to fill, so the rest of the exposure is covered by a further
/// corrective. A mandate that has already been corrected once stays able to make
/// progress.
#[test]
fn a_part_filled_then_finalized_corrective_is_followed_up() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
        batch(
            vec![event("V-103", "pex-3", 4, at(OPEN + 130))],
            vec![block("bB", &["V-103"], 0, at(OPEN + 130))],
            "k3",
        ),
    ]);
    let mut engine = rig.engine();

    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    rig.cycle(&mut engine, OPEN);
    rig.cycle(&mut engine, OPEN + 60);
    rig.cycle(&mut engine, OPEN + 120);
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: a cancelled leg is covered for exactly the quantity it is short"
    );

    rig.cycle(&mut engine, OPEN + 180);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 6],
        "FAILED: a finalized neutralization contributes no still-working \
         quantity, so its fill resolves 4 of the 10 and the remaining 6 is \
         covered by a further corrective"
    );
}

/// The open-order list and the fill feed report the same running total about one
/// order. Folding both in adds the same base units twice, which here would make
/// a short leg look met and suppress the corrective entirely.
#[test]
fn the_open_order_list_and_the_feed_describe_one_quantity() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(
            vec![
                event("V-101", "pex-1", 6, at(OPEN)),
                event("V-102", "pex-2", 10, at(OPEN)),
            ],
            vec![],
            "k1",
        ),
    ]);
    let mut engine = rig.engine();

    rig.gateway.set_open(vec![
        open_order("V-101", "pex-1", 101, 10, 6),
        open_order("V-102", "pex-2", 102, 10, 0),
    ]);
    rig.cycle(&mut engine, OPEN);
    rig.cycle(&mut engine, OPEN + 60);
    rig.cycle(&mut engine, OPEN + 120);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![4],
        "FAILED: filled_qty and cumulative_filled_qty are the same total seen \
         from two sides - the North leg is 6 short, not met"
    );
}

/// The window is tested against the venue's match timestamp, not against the
/// cycle that happened to read the event. An event stamped exactly at the end of
/// the window names an identifier the venue may already have recycled.
#[test]
fn a_match_at_the_window_end_is_not_attributed() {
    let window_end = OPEN + 900;
    let rig = Rig::new(vec![
        lead_in(),
        batch(
            vec![
                event("V-101", "pex-1", 10, at(window_end)),
                event("V-102", "pex-2", 10, at(window_end)),
            ],
            vec![],
            "k1",
        ),
    ]);
    let mut engine = rig.engine();

    rig.cycle(&mut engine, OPEN);
    rig.cycle(&mut engine, window_end);

    assert!(
        engine.positions().is_empty(),
        "FAILED: the attribution window is half-open - a match stamped at \
         window_start + ATTRIBUTION_WINDOW is outside it and is attributed to \
         nothing"
    );
    assert!(
        rig.gateway.corrective_sizes().is_empty(),
        "FAILED: neither leg acquired anything, so there is no exposure to \
         correct"
    );
}

/// A lost acknowledgement survives a restart: the record keeps its client
/// reference, and the retry after the restart reuses it so the venue's
/// idempotency resolves it onto the original order rather than placing a second
/// economic order.
#[test]
fn an_unacknowledged_corrective_survives_a_restart_and_retries_under_its_ref() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
    ]);
    rig.gateway
        .fail_on(3, SubmitError::AcknowledgementLost);

    {
        let mut engine = rig.engine();
        rig.gateway
            .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
        rig.cycle(&mut engine, OPEN);
        rig.cycle(&mut engine, OPEN + 60);
        rig.cycle(&mut engine, OPEN + 120);
    }

    let saved = rig.store.snapshot();
    assert_eq!(
        saved.neutralizations.len(),
        1,
        "FAILED: a committed neutralization is durable state, resolved or not \
         - an unacknowledged one is the whole reason the client reference has \
         to survive"
    );
    assert_eq!(
        saved.neutralizations[0].client_ref.as_str(),
        "pex-3",
        "FAILED: the client reference a retry depends on has to survive a \
         restart"
    );
    assert!(
        saved.submissions
            .iter()
            .any(|record| record.client_ref.as_str() == "pex-3"),
        "FAILED: an order the engine has committed to is tracked from the \
         moment it is committed to, acknowledged or not"
    );
    let window_start = saved
        .submissions
        .iter()
        .find(|record| record.client_ref.as_str() == "pex-3")
        .map(|record| record.window_start);
    assert_eq!(
        window_start,
        Some(Timestamp::from_seconds(OPEN + 120)),
        "FAILED: a restart must not reopen an attribution window - the \
         original window start is what a later observation is tested against"
    );

    // Restart, and let the retry land.
    let mut engine = rig.engine();
    rig.gateway.set_open(vec![]);
    rig.cycle(&mut engine, OPEN + 180);

    assert_eq!(
        rig.gateway.corrective_refs(),
        vec!["pex-3", "pex-3"],
        "FAILED: a lost acknowledgement is retried under the same client \
         reference, before and after a restart alike - a freshly minted \
         reference would ask the venue for a second order"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10, 10],
        "FAILED: two attempts under one client reference are one economic \
         order to the venue, not two - and no third exposure is sent for an \
         order that may already exist"
    );
}

/// Two mandates may name the same instrument. Quantity acquired for one must not
/// complete a leg of the other, and only the mandate that is genuinely one-legged
/// draws a corrective.
#[test]
fn shared_instruments_do_not_cross_attribute() {
    let shared = InstrumentId::new("shared-north");
    let strategies = FakeStrategies::new(vec![
        StrategyCandidate::new(
            StrategyId::new("mA"),
            shared.clone(),
            InstrumentId::new("south-a"),
            0.45,
            0.45,
            30,
            30,
            at(10_000),
        ),
        StrategyCandidate::new(
            StrategyId::new("mB"),
            shared.clone(),
            InstrumentId::new("south-b"),
            0.45,
            0.45,
            30,
            30,
            at(10_000),
        ),
    ]);
    let clock = FakeClock::new(at(OPEN));
    let inventory = FakeInventory::new(1_000.0);
    let gateway = FakeGateway::new();
    let fills = FakeFills::new(vec![
        lead_in(),
        batch(
            vec![
                event("V-101", "pex-1", 30, at(OPEN)),
                event("V-103", "pex-3", 30, at(OPEN)),
            ],
            vec![],
            "k1",
        ),
    ]);
    let store = FakeStore::empty();
    let mut engine = Engine::new(
        EngineConfig::default(),
        &strategies,
        &inventory,
        &gateway,
        &fills,
        &store,
        &clock,
    );

    // Both mandates are already fully held in custody on the shared identifier.
    inventory.set_instrument(shared.clone(), 60);

    clock.set(at(OPEN));
    engine.run_cycle().expect("cycle 1");
    clock.set(at(OPEN + 60));
    engine.run_cycle().expect("cycle 2");

    // Submissions 1 and 2 are mA's legs, 3 and 4 are mB's. Only mA's North leg
    // (V-101) filled, so only mA is one-legged.
    let completed: Vec<String> = engine
        .positions()
        .iter()
        .map(|position| position.strategy_id.to_string())
        .collect();
    assert!(
        !completed.iter().any(|id| id == "mB"),
        "FAILED: custody inventory aggregates across mandates and cannot prove \
         that one particular order filled - mB's 30 units belong to mA"
    );

    let south_takers: Vec<i64> = gateway
        .submitted
        .borrow()
        .iter()
        .filter(|request| request.kind == OrderKind::ImmediateOrCancel)
        .filter(|request| request.strategy_id.as_str() == "mA")
        .map(|request| request.size)
        .collect();
    assert!(
        south_takers.is_empty(),
        "FAILED: mA's South leg is still unconfirmed, and an unconfirmed leg is \
         never neutralized against"
    );
}

/// The corrective follows the cancellation of the resting order, never precedes
/// it: a fill already in flight has to get a cycle in which to land before
/// another economic order is sent.
#[test]
fn the_closure_of_the_resting_order_precedes_the_corrective() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
    ]);
    let mut engine = rig.engine();

    rig.gateway
        .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
    rig.cycle(&mut engine, OPEN);
    rig.cycle(&mut engine, OPEN + 60);

    assert!(
        rig.gateway.cancelled.borrow().is_empty()
            && rig.gateway.corrective_sizes().is_empty(),
        "FAILED: the first cycle moves the resting order into closure and sends \
         nothing - a resting order may not be treated as gone merely because a \
         correction has become necessary"
    );

    rig.cycle(&mut engine, OPEN + 120);

    assert_eq!(
        rig.gateway.cancelled.borrow().len(),
        1,
        "FAILED: the resting order is cancelled at the venue before any \
         corrective is issued"
    );
    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: once the cancellation boundary has passed, the residual is \
         covered in the same cycle"
    );
}

/// A restart in the middle of a closure must not lose the sequence: the resting
/// order is still cancelled before the corrective goes out, and the mandate is
/// not left to start marking itself all over again on the far side.
#[test]
fn a_restart_in_the_middle_of_a_closure_keeps_the_sequence() {
    let rig = Rig::new(vec![
        lead_in(),
        batch(vec![event("V-101", "pex-1", 10, at(OPEN))], vec![], "k1"),
        batch(vec![], vec![], "k2"),
    ]);
    {
        let mut engine = rig.engine();
        rig.gateway
            .set_open(vec![open_order("V-102", "pex-2", 102, 10, 0)]);
        rig.cycle(&mut engine, OPEN);
        rig.cycle(&mut engine, OPEN + 60);
    }

    // Restart with the south leg still resting at the venue.
    let mut engine = rig.engine();
    rig.cycle(&mut engine, OPEN + 120);

    assert_eq!(
        rig.gateway.corrective_sizes(),
        vec![10],
        "FAILED: work in progress survives a restart - a leg already moved into \
         closure is cancelled and covered rather than marked again from scratch"
    );
    assert_eq!(
        rig.gateway.cancelled.borrow().len(),
        1,
        "FAILED: the resting order is cancelled at the venue before the \
         corrective is issued, across a restart as within one run"
    );
}
