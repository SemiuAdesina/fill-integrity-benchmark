//! Diagnostics: state that must not outlive the order it belongs to.
//!
//! The venue recycles identifiers once a window has closed, so durable state keyed
//! by an identifier speaks only for the order that held it while its window was
//! open. Each scenario drives the engine through its public surface against the
//! in-memory venue in `harness/` and reports what it observed. None of this is
//! graded.

mod harness;

use harness::*;

/// One mandate closes its South leg and settles. The venue then hands that
/// identifier to a later mandate's North leg.
#[test]
fn a_recycled_identifier_does_not_inherit_closure_state() {
    let outcome = run(
        Scenario::new(vec![candidate("fa", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("tA"), 0.54)
            .ask(instrument("tB"), 0.54)
            .with_cycle_seconds(60)
            .step(vec![])
            .step(vec![fill_resting("fa", Venue::North, 30)])
            .idle_steps(15)
            .step(vec![
                Action::AddCandidate { candidate: candidate("fb", "nB", "tB", 0.45, 0.52, 20, 20) },
                Action::RecycleClosedOrderId {
                    from: OrderRef::Resting { strategy: strategy("fa"), venue: Venue::South },
                },
            ])
            .step(vec![fill_resting("fb", Venue::South, 20)])
            .idle_steps(8),
    );
    let fb_north = outcome.resting_for("fb", Venue::North);
    assert_eq!(fb_north.len(), 1, "fb placed {} resting North legs", fb_north.len());
    let recycled = fb_north[0].order_id.as_str().to_string();
    let cancels = outcome
        .cancelled_order_ids
        .iter()
        .filter(|id| id.as_str() == recycled)
        .count();
    // Once for the order that held the identifier first, once for the one that
    // wears it now: the new order still has to be taken off the venue before its
    // residual is covered.
    assert_eq!(
        cancels, 2,
        "the order now wearing a recycled identifier was cancelled {} times; closure state keyed \
         by the identifier would make it inherit the dead order's",
        cancels
    );
    assert_eq!(
        taker_sizes(&outcome, "fb", Venue::North),
        vec![20],
        "corrective orders {:?} for the leg wearing the recycled identifier",
        taker_sizes(&outcome, "fb", Venue::North)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["fa".to_string(), "fb".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A block stamped outside every covered order's window is evidence about some
/// other order: it allocates nothing, yet it still closes them all.
#[test]
fn a_block_outside_every_window_finalizes_without_allocating() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ga", "nA", "tG", 0.45, 0.52, 10, 10),
            candidate("gb", "nB", "tG", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tG"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![
            fill_resting("ga", Venue::North, 10),
            fill_resting("gb", Venue::North, 20),
        ])
        .step(vec![])
        .step(vec![block_takers_at(&["ga", "gb"], Venue::South, 25, 0.55, 2_000)])
        .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "ga", Venue::South),
        vec![10, 10],
        "a block outside the window allocates nothing, and closes the orders it names: \
         corrective orders {:?}",
        taker_sizes(&outcome, "ga", Venue::South)
    );
    assert_eq!(
        taker_sizes(&outcome, "gb", Venue::South),
        vec![20, 20],
        "corrective orders {:?}",
        taker_sizes(&outcome, "gb", Venue::South)
    );
    assert!(
        outcome.positions.is_empty(),
        "a block outside every window cannot complete a leg, yet positions were recorded"
    );
}

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
