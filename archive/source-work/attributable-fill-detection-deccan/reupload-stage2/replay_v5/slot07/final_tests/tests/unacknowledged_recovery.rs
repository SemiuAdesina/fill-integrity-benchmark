//! Diagnostics: orders the venue may have created but never named.
//!
//! `AcknowledgementLost` applies to every order the engine submits, an entry leg
//! as much as a corrective one, and the `client_ref` identity a retry depends on
//! has to survive a restart. Each scenario drives the engine through its public
//! surface against the in-memory venue in `harness/` and reports what it
//! observed. None of this is graded.

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

fn taker_sizes(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Vec<Qty> {
    outcome
        .takers_for(strategy_id, venue)
        .into_iter()
        .map(|order| order.size)
        .collect()
}

/// The South entry leg reaches the venue and rests there, but its
/// acknowledgement is lost. Nothing but the client reference ties it back.
#[test]
fn an_entry_leg_whose_acknowledgement_was_lost_is_recovered_not_replaced() {
    let outcome = run(
        Scenario::new(vec![candidate("ea", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![fill_resting("ea", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_for("ea", Venue::South).len(),
        1,
        "the venue holds {} resting South legs after one acknowledgement was lost; a retry \
         under the same client reference reattaches to the original order",
        outcome.resting_for("ea", Venue::South).len()
    );
    assert_eq!(
        taker_sizes(&outcome, "ea", Venue::South),
        vec![30],
        "corrective orders {:?} for a leg of 30 that never filled",
        taker_sizes(&outcome, "ea", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ea".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// The same lost entry-leg acknowledgement, with a restart before the retry. The
/// reference the retry depends on is only durable if it was persisted.
#[test]
fn an_unacknowledged_entry_leg_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("ha", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![Action::Restart])
            .step(vec![fill_resting("ha", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_for("ha", Venue::South).len(),
        1,
        "after a restart the unacknowledged entry leg was placed a second time: {} resting \
         South legs at the venue",
        outcome.resting_for("ha", Venue::South).len()
    );
    assert_eq!(
        taker_sizes(&outcome, "ha", Venue::South),
        vec![30],
        "corrective orders {:?}",
        taker_sizes(&outcome, "ha", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ha".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A committed correction whose acknowledgement was lost, with a restart before
/// it resolves.
#[test]
fn an_unacknowledged_correction_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("ia", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("ia", Venue::North, 30)])
            .step(vec![Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .step(vec![fill_taker("ia", Venue::South, 30)])
            .idle_steps(5),
    );
    assert_eq!(
        taker_sizes(&outcome, "ia", Venue::South),
        vec![30],
        "the venue holds corrective orders {:?} for one exposure across a restart",
        taker_sizes(&outcome, "ia", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ia".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A failed poll delivered nothing, so the cursor must not move past the events
/// it never carried.
#[test]
fn a_failed_poll_does_not_skip_the_events_it_never_delivered() {
    let outcome = run(
        Scenario::new(vec![candidate("ja", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.60)
            .step(vec![])
            .step(vec![Action::FailNextFillPoll, fill_resting("ja", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "ja", Venue::South),
        vec![30],
        "the fill the failed poll never delivered was lost: corrective orders {:?}",
        taker_sizes(&outcome, "ja", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ja".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}
