//! Diagnostics: what a restart must not change.
//!
//! Each scenario drives the engine through its public surface against the
//! in-memory venue in `harness/` and reports what it observed. A failure names
//! the observable outcome that was wrong, in the terms of `/app/spec`. None of
//! this is graded.

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

/// North fills, a corrective order goes out, the process restarts twice and the
/// whole feed is delivered again.
#[test]
fn a_restart_does_not_redo_settled_work() {
    let outcome = run(
        Scenario::new(vec![candidate("ra", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ra", Venue::North, 30)])
            .idle_steps(2)
            .step(vec![Action::Restart])
            .step(vec![Action::Restart, Action::RedeliverAllFillEvents])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        1,
        "{} corrective orders exist for one exposure after two restarts and a feed replay",
        outcome.taker_orders().len()
    );
    assert_eq!(position_ids(&outcome), vec!["ra".to_string()], "positions {:?}", position_ids(&outcome));
}

/// North's fill is consumed from the feed and the order leaves the live list;
/// then the process restarts. No live source can supply that fill again.
#[test]
fn attribution_no_live_source_can_resupply_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("rb", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("rb", Venue::North, 30)])
            .step(vec![Action::WithholdFillEvents, Action::Restart])
            .idle_steps(3)
            .step(vec![Action::ReleaseFillEvents])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.taker_qty_on_instrument("tA"),
        30,
        "after a restart the North fill of 30 was lost: corrective quantity {} on the South leg",
        outcome.taker_qty_on_instrument("tA")
    );
    assert_eq!(position_ids(&outcome), vec!["rb".to_string()], "positions {:?}", position_ids(&outcome));
}

/// One mandate settles before a restart; another enters after it.
#[test]
fn client_references_minted_after_a_restart_are_new_to_the_venue() {
    let outcome = run(
        Scenario::new(vec![candidate("rc", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("tA"), 0.54)
            .ask(instrument("tB"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("rc", Venue::North, 30)])
            .idle_steps(3)
            .step(vec![Action::Restart, Action::AddCandidate { candidate: candidate("rd", "nB", "tB", 0.45, 0.52, 20, 20) }])
            .step(vec![fill_resting("rd", Venue::North, 20)])
            .idle_steps(6),
    );
    let reused: Vec<&SubmitAttempt> = outcome
        .attempts
        .iter()
        .filter(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_)))
        .collect();
    assert!(
        reused.is_empty(),
        "after a restart the engine submitted under a client reference the venue already knew, \
         and the venue answered with the existing order instead of creating one: {:?}",
        reused.iter().map(|attempt| (attempt.strategy_id.as_str(), attempt.venue, attempt.kind)).collect::<Vec<_>>()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["rc".to_string(), "rd".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A block is absorbed, the process restarts, and the feed delivers the block
/// again.
#[test]
fn a_block_absorbed_before_a_restart_is_not_counted_again_after_it() {
    let outcome = run(
        Scenario::new(vec![
            candidate("re", "nA", "tR", 0.45, 0.52, 10, 10),
            candidate("rf", "nB", "tR", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tR"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("re", Venue::North, 10), fill_resting("rf", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers(&["rf", "re"], Venue::South, 24, 0.55)])
        .step(vec![Action::Restart, Action::RedeliverAllBlocks])
        .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        4,
        "a block of 24 over 10 and 20 delivered again after a restart: {} corrective orders \
         exist where one division and one follow-up per order would give 4",
        outcome.taker_orders().len()
    );
    assert!(
        outcome.positions.is_empty(),
        "a block of 24 cannot complete legs totalling 30, yet positions were recorded"
    );
}
