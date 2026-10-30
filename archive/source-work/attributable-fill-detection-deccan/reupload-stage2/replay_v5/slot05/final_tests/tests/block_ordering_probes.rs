//! Extra probes: parts of the contract the six shipped suites do not exercise.
//!
//! These drive the same public surface and the same in-memory venue. They exist
//! to pin a reading of `/app/spec/venue-semantics.md` and
//! `/app/spec/reconciliation-contract.md`; like the shipped suites, none of them
//! is graded.

mod harness;

use harness::*;

fn taker_sizes(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Vec<Qty> {
    outcome
        .takers_for(strategy_id, venue)
        .into_iter()
        .map(|order| order.size)
        .collect()
}

fn position_ids(outcome: &Outcome) -> Vec<String> {
    let mut ids: Vec<String> = outcome
        .positions
        .iter()
        .map(|position| position.strategy_id.as_str().to_string())
        .collect();
    ids.sort();
    ids
}

/// Two corrective orders on one instrument, accepted in different venue ticks.
///
/// The venue's opaque labels sort the later one first, so a division that leans
/// on the identifier gets the remainder onto the wrong order. The rule orders by
/// the start of the attribution window first.
#[test]
fn a_block_orders_covered_orders_by_window_start_not_by_identifier() {
    let outcome = run(
        Scenario::new(vec![
            candidate("pa", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("pb", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("pa", Venue::North, 10)])
        .step(vec![])
        .step(vec![fill_resting("pb", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["pb", "pa"], Venue::South, 11, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pa", Venue::South), taker_sizes(&outcome, "pb", Venue::South)),
        (vec![10, 4], vec![10, 5]),
        "a block of 11 over two corrective orders of 10 accepted in different ticks left \
         residuals of {:?} and {:?}; the remainder goes to the order whose window opened first",
        taker_sizes(&outcome, "pa", Venue::South),
        taker_sizes(&outcome, "pb", Venue::South)
    );
}

/// A block whose match instant is outside every covered order's window. Nothing
/// is allocated, and the venue has still closed every order it named.
#[test]
fn a_block_stamped_outside_every_window_allocates_nothing_and_still_closes() {
    let window = ATTRIBUTION_WINDOW.as_seconds();
    let outcome = run(
        Scenario::new(vec![
            candidate("pc", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("pd", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("pc", Venue::North, 10), fill_resting("pd", Venue::North, 10)])
        .step(vec![])
        .step(vec![])
        .step(vec![block_takers_at(&["pc", "pd"], Venue::South, 12, 0.55, -(window + 100))])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pc", Venue::South), taker_sizes(&outcome, "pd", Venue::South)),
        (vec![10, 10], vec![10, 10]),
        "a block from outside both attribution windows was allocated: pc {:?}, pd {:?}",
        taker_sizes(&outcome, "pc", Venue::South),
        taker_sizes(&outcome, "pd", Venue::South)
    );
    assert!(
        outcome.positions.is_empty(),
        "a block that allocated nothing completed a mandate: {:?}",
        position_ids(&outcome)
    );
}

/// Quantity above the basis is not attributable to anything the engine tracks
/// and is dropped rather than invented.
#[test]
fn a_block_larger_than_the_basis_is_capped_at_it() {
    let outcome = run(
        Scenario::new(vec![
            candidate("pe", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("pf", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("pe", Venue::North, 10), fill_resting("pf", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["pe", "pf"], Venue::South, 100, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pe", Venue::South), taker_sizes(&outcome, "pf", Venue::South)),
        (vec![10], vec![10]),
        "a block of 100 over outstanding 10 and 10 left corrective orders pe {:?}, pf {:?}",
        taker_sizes(&outcome, "pe", Venue::South),
        taker_sizes(&outcome, "pf", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pe".to_string(), "pf".to_string()],
        "both legs were covered by the block, yet positions are {:?}",
        position_ids(&outcome)
    );
}

/// A poll that fails must leave the cursor where it was: the events it never
/// delivered still have to arrive.
#[test]
fn a_failed_poll_does_not_consume_the_events_it_never_delivered() {
    let outcome = run(
        Scenario::new(vec![candidate("pg", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("pg", Venue::North, 30), Action::FailNextFillPoll])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "pg", Venue::South),
        vec![30],
        "a fill whose poll failed once was corrected as {:?}",
        taker_sizes(&outcome, "pg", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["pg".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A leg marked for closure, then a restart before the cancellation lands. The
/// closure has to survive, and the correction must still be sent exactly once.
#[test]
fn closure_in_progress_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("ph", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("ph", Venue::North, 30)])
            .step(vec![Action::Restart])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "ph", Venue::South),
        vec![30],
        "after a restart mid-closure the corrective orders are {:?}",
        taker_sizes(&outcome, "ph", Venue::South)
    );
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        1,
        "{} cancellations were sent for one resting leg",
        outcome.cancelled_order_ids.len()
    );
}

/// A corrective order committed before a restart, whose acknowledgement was
/// lost, is retried under its own reference afterwards rather than placed again.
#[test]
fn a_committed_correction_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("pi", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pi", Venue::North, 30)])
            .step(vec![])
            .step(vec![Action::Restart, Action::LoseNextAcknowledgement])
            .idle_steps(4)
            .step(vec![fill_taker("pi", Venue::South, 30)])
            .idle_steps(3),
    );
    assert_eq!(
        taker_sizes(&outcome, "pi", Venue::South),
        vec![30],
        "the venue holds corrective orders {:?} for one exposure",
        taker_sizes(&outcome, "pi", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["pi".to_string()], "positions {:?}", position_ids(&outcome));
}

/// The venue refuses every cancellation. The leg stays live throughout, so no
/// correction is ever sent against quantity the resting order may still acquire.
#[test]
fn a_refused_cancellation_does_not_release_a_correction() {
    let outcome = run(
        Scenario::new(vec![candidate("pj", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pj", Venue::North, 30)])
            .step(vec![Action::FailNextCancel])
            .step(vec![Action::FailNextCancel])
            .step(vec![Action::FailNextCancel]),
    );
    assert!(
        outcome.takers_for("pj", Venue::South).is_empty(),
        "a correction was sent while the resting leg was still live: {:?}",
        taker_sizes(&outcome, "pj", Venue::South)
    );
    assert!(
        outcome.cancelled_order_ids.is_empty(),
        "the venue refused every cancellation, yet one is recorded as having landed"
    );
}

/// Cancellation fails for two cycles and then succeeds. A refused cancellation
/// does not latch the mandate shut: the correction follows once the leg is off
/// the book.
#[test]
fn a_correction_follows_once_a_retried_cancellation_lands() {
    let outcome = run(
        Scenario::new(vec![candidate("pk", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pk", Venue::North, 30)])
            .step(vec![Action::FailNextCancel])
            .step(vec![Action::FailNextCancel])
            .idle_steps(5),
    );
    assert_eq!(
        taker_sizes(&outcome, "pk", Venue::South),
        vec![30],
        "after two refused cancellations the corrective orders are {:?}",
        taker_sizes(&outcome, "pk", Venue::South)
    );
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        1,
        "{} cancellations landed for one resting leg",
        outcome.cancelled_order_ids.len()
    );
}
