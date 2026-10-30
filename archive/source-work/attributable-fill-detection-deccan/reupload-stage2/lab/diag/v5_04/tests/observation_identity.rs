//! Diagnostics: which order an observation is about.
//!
//! Each scenario drives the engine through its public surface against the
//! in-memory venue in `harness/` and reports what it observed. A failure names
//! the observable outcome that was wrong, in the terms of `/app/spec`. None of
//! this is graded; it is a working set for checking a reading of the contract.

mod harness;

use harness::*;

fn position_ids(outcome: &Outcome) -> Vec<String> {
    let mut ids: Vec<String> = outcome
        .positions
        .iter()
        .map(|position| position.strategy_id.as_str().to_string())
        .collect();
    ids.sort();
    ids
}

/// The corrective order's acknowledgement is lost. It exists at the venue and
/// fills there, and the only thing that ties its fill event back to the engine
/// is the client reference the engine chose.
#[test]
fn a_fill_carrying_the_client_reference_of_an_unacknowledged_order_reaches_it() {
    let outcome = run(
        Scenario::new(vec![candidate("ia", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![Action::LoseNextAcknowledgement, fill_resting("ia", Venue::North, 30)])
            .idle_steps(6),
    );
    let takers = outcome.takers_for("ia", Venue::South);
    assert_eq!(
        takers.len(),
        1,
        "the venue holds {} corrective orders for one exposure after one acknowledgement was lost",
        takers.len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ia".to_string()],
        "the corrective order filled at the venue, yet the mandate never completed: \
         positions {:?}",
        position_ids(&outcome)
    );
}

/// The venue recycles the identifier of a cancelled order once its attribution
/// window has passed. A later mandate's leg wears it, and that leg's fill event
/// names the identifier together with the later order's own client reference.
#[test]
fn a_recycled_identifier_belongs_to_the_order_that_holds_it_now() {
    let outcome = run(
        Scenario::new(vec![candidate("ia", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("tA"), 0.54)
            .ask(instrument("tB"), 0.54)
            .with_cycle_seconds(60)
            .step(vec![])
            .step(vec![fill_resting("ia", Venue::North, 30)])
            .idle_steps(15)
            .step(vec![
                Action::AddCandidate { candidate: candidate("ib", "nB", "tB", 0.45, 0.52, 20, 20) },
                Action::RecycleClosedOrderId {
                    from: OrderRef::Resting { strategy: strategy("ia"), venue: Venue::South },
                },
            ])
            .step(vec![fill_resting("ib", Venue::North, 20), fill_resting("ib", Venue::South, 20)])
            .idle_steps(6),
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ia".to_string(), "ib".to_string()],
        "both legs of the mandate whose order wears a recycled identifier filled, yet the \
         positions recorded are {:?}",
        position_ids(&outcome)
    );
    assert!(
        outcome.takers_for("ib", Venue::North).is_empty() && outcome.takers_for("ib", Venue::South).is_empty(),
        "a corrective order was sent for a leg that filled: {:?}",
        outcome
            .taker_orders()
            .iter()
            .map(|order| (order.strategy_id.as_str().to_string(), order.venue, order.size))
            .collect::<Vec<_>>()
    );
}

/// Two mandates rest on the same instrument. Only one of them fills, and the
/// custody account cannot say which.
#[test]
fn a_fill_on_a_shared_instrument_belongs_to_one_order_only() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ic", "nS", "tC", 0.45, 0.52, 25, 25),
            candidate("id", "nS", "tD", 0.44, 0.53, 25, 25),
        ])
        .ask(instrument("nS"), 0.47)
        .ask(instrument("tC"), 0.54)
        .ask(instrument("tD"), 0.55)
        .step(vec![])
        .step(vec![fill_resting("id", Venue::North, 25)])
        .idle_steps(6),
    );
    assert!(
        outcome.position("ic").is_none(),
        "a mandate whose orders never filled recorded a pairing"
    );
    assert!(
        outcome.takers_on_instrument("tC").is_empty(),
        "a corrective order was sent for a mandate none of whose orders filled"
    );
    assert_eq!(
        outcome.takers_on_instrument("tD").len(),
        1,
        "the mandate whose North leg filled drew {} corrective orders on its South leg",
        outcome.takers_on_instrument("tD").len()
    );
}
