//! Diagnostics: closing a one-legged mandate's exposure.
//!
//! Each scenario drives the engine through its public surface against the
//! in-memory venue in `harness/` and reports what it observed. A failure names
//! the observable outcome that was wrong, in the terms of `/app/spec`. None of
//! this is graded.

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

/// North fills in full, South fills 7 of 30.
#[test]
fn a_one_legged_mandate_is_corrected_for_its_residual_only() {
    let outcome = run(
        Scenario::new(vec![candidate("na", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.60)
            .step(vec![])
            .step(vec![fill_resting("na", Venue::North, 30)])
            .step(vec![fill_resting("na", Venue::South, 7)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "na", Venue::South),
        vec![23],
        "corrective orders {:?} were sent for a leg of 30 that had filled 7",
        taker_sizes(&outcome, "na", Venue::South)
    );
}

/// The corrective order rests unfilled at the venue for several cycles.
#[test]
fn quantity_already_working_in_a_corrective_order_is_not_corrected_again() {
    let outcome = run(
        Scenario::new(vec![candidate("nb", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("nb", Venue::North, 30)])
            .idle_steps(5)
            .step(vec![fill_taker("nb", Venue::South, 30)])
            .idle_steps(3),
    );
    assert_eq!(
        taker_sizes(&outcome, "nb", Venue::South),
        vec![30],
        "corrective orders {:?} were sent for one exposure of 30 while the first was still \
         working at the venue",
        taker_sizes(&outcome, "nb", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["nb".to_string()], "positions {:?}", position_ids(&outcome));
}

/// The venue refuses the first corrective order outright.
#[test]
fn a_refused_correction_leaves_the_exposure_open_for_a_later_attempt() {
    let outcome = run(
        Scenario::new(vec![candidate("nc", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("nc", Venue::North, 30), Action::RejectNextTakerSubmission])
            .idle_steps(8),
    );
    assert_eq!(
        outcome.takers_for("nc", Venue::South).len(),
        1,
        "after the venue refused the first corrective order, {} corrective orders exist at the \
         venue; the exposure needs exactly one",
        outcome.takers_for("nc", Venue::South).len()
    );
    assert_eq!(position_ids(&outcome), vec!["nc".to_string()], "positions {:?}", position_ids(&outcome));
}

/// The corrective order reaches the venue and rests there, but its
/// acknowledgement is lost.
#[test]
fn a_lost_acknowledgement_is_retried_under_the_same_client_reference() {
    let outcome = run(
        Scenario::new(vec![candidate("nd", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("nd", Venue::North, 30), Action::LoseNextAcknowledgement])
            .idle_steps(3)
            .step(vec![fill_taker("nd", Venue::South, 30)])
            .idle_steps(3),
    );
    let takers = outcome.takers_for("nd", Venue::South);
    assert_eq!(
        takers.len(),
        1,
        "the venue holds {} corrective orders for one exposure after one acknowledgement was lost",
        takers.len()
    );
    let attempts: Vec<&SubmitAttempt> = outcome
        .attempts
        .iter()
        .filter(|attempt| attempt.kind == OrderKind::ImmediateOrCancel)
        .collect();
    assert!(
        attempts.iter().any(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_))),
        "the corrective order whose acknowledgement was lost was never retried under its own \
         client reference: submissions {:?}",
        attempts.iter().map(|attempt| &attempt.outcome).collect::<Vec<_>>()
    );
    assert_eq!(position_ids(&outcome), vec!["nd".to_string()], "positions {:?}", position_ids(&outcome));
}

/// The South leg fills while the feed is withheld: it has left the live list,
/// and nothing the engine can see says why.
#[test]
fn a_leg_absent_from_the_live_list_that_the_engine_did_not_cancel_is_left_alone() {
    let outcome = run(
        Scenario::new(vec![candidate("ne", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ne", Venue::North, 30)])
            .step(vec![Action::WithholdFillEvents, fill_resting("ne", Venue::South, 30)])
            .idle_steps(3)
            .step(vec![Action::ReleaseFillEvents])
            .idle_steps(4),
    );
    assert!(
        outcome.takers_on_instrument("tA").is_empty(),
        "a corrective order was sent for a leg that had left the live list without the engine \
         cancelling it, and which turned out to have filled: {:?}",
        taker_sizes(&outcome, "ne", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["ne".to_string()], "positions {:?}", position_ids(&outcome));
}

/// The venue nets both resting South legs into one block of 11 and closes them.
#[test]
fn a_leg_the_venue_closed_in_a_block_is_corrected_for_what_the_block_did_not_fill() {
    let outcome = run(
        Scenario::new(vec![
            candidate("nf", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("ng", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![])
        .step(vec![
            fill_resting("nf", Venue::North, 10),
            fill_resting("ng", Venue::North, 10),
            block_orders(
                vec![
                    OrderRef::Resting { strategy: strategy("nf"), venue: Venue::South },
                    OrderRef::Resting { strategy: strategy("ng"), venue: Venue::South },
                ],
                11,
                0.53,
                0,
            ),
        ])
        .idle_steps(8),
    );
    let sizes = (taker_sizes(&outcome, "nf", Venue::South), taker_sizes(&outcome, "ng", Venue::South));
    assert!(
        sizes.0.len() == 1 && sizes.1.len() == 1 && sizes.0[0] + sizes.1[0] == 9,
        "the venue closed both resting legs in a block of 11 over 10 and 10; the residual of 9 \
         base units was corrected as {:?}",
        sizes
    );
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        0,
        "{} cancellations were sent for orders the venue had already closed",
        outcome.cancelled_order_ids.len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["nf".to_string(), "ng".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}
