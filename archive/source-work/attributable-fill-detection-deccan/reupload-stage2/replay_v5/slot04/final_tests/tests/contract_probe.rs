//! Extra probes for corners of the contract the six suites do not reach.
//!
//! Same surface, same in-memory venue. Not graded; a working set for holding the
//! engine to the whole of `/app/spec/reconciliation-contract.md`.

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

/// An entry leg whose acknowledgement was lost is still one order at the venue,
/// and the mandate still completes.
#[test]
fn an_entry_leg_whose_acknowledgement_was_lost_is_not_placed_twice() {
    let outcome = run(
        Scenario::new(vec![candidate("pa", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![fill_resting("pa", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_for("pa", Venue::South).len(),
        1,
        "the venue holds {} resting South legs for one mandate after an acknowledgement was lost",
        outcome.resting_for("pa", Venue::South).len()
    );
    assert_eq!(
        taker_sizes(&outcome, "pa", Venue::South),
        vec![30],
        "corrective orders for the leg that never filled are {:?}",
        taker_sizes(&outcome, "pa", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["pa".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A cancel that fails leaves the leg resting; the correction waits for the
/// cancellation boundary rather than doubling the exposure.
#[test]
fn a_failed_cancellation_defers_the_correction() {
    let outcome = run(
        Scenario::new(vec![candidate("pb", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("pb", Venue::North, 30)])
            .step(vec![Action::FailNextCancel])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "pb", Venue::South),
        vec![30],
        "corrective orders after one cancel failed are {:?}",
        taker_sizes(&outcome, "pb", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["pb".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A block whose match instant falls outside every covered order's window is
/// evidence about some other order: it allocates nothing, and closes them anyway.
#[test]
fn a_block_outside_every_window_allocates_nothing_and_still_closes() {
    let outcome = run(
        Scenario::new(vec![
            candidate("pc", "nA", "tP", 0.45, 0.52, 10, 10),
            candidate("pd", "nB", "tP", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tP"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("pc", Venue::North, 10), fill_resting("pd", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers_at(&["pc", "pd"], Venue::South, 15, 0.55, 1_000)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pc", Venue::South), taker_sizes(&outcome, "pd", Venue::South)),
        (vec![10, 10], vec![20, 20]),
        "a block stamped outside every window allocated nothing and closed both orders, so each \
         leg's full size is still outstanding; corrective orders are pc {:?}, pd {:?}",
        taker_sizes(&outcome, "pc", Venue::South),
        taker_sizes(&outcome, "pd", Venue::South)
    );
    assert!(outcome.positions.is_empty(), "a block that allocated nothing completed a mandate");
}

/// A block total above the basis is not attributable to anything the engine
/// tracks, so the excess is dropped rather than invented.
#[test]
fn quantity_above_the_basis_is_dropped() {
    let outcome = run(
        Scenario::new(vec![
            candidate("pe", "nA", "tK", 0.45, 0.52, 10, 10),
            candidate("pf", "nB", "tK", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tK"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("pe", Venue::North, 10), fill_resting("pf", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers(&["pe", "pf"], Venue::South, 500, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pe", Venue::South), taker_sizes(&outcome, "pf", Venue::South)),
        (vec![10], vec![20]),
        "a block of 500 over outstanding 10 and 20 fills both legs; corrective orders are \
         pe {:?}, pf {:?}",
        taker_sizes(&outcome, "pe", Venue::South),
        taker_sizes(&outcome, "pf", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pe".to_string(), "pf".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A match stamped before the window opened is not this order's either.
#[test]
fn a_match_stamped_before_the_window_opened_is_not_attributed() {
    let outcome = run(
        Scenario::new(vec![candidate("pg", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![
                Action::SetFillEventTimeOffset { seconds: -30 },
                fill_resting("pg", Venue::North, 30),
            ])
            .idle_steps(6),
    );
    assert!(
        outcome.positions.is_empty() && outcome.taker_orders().is_empty(),
        "an observation stamped before window_start was counted: positions {}, corrective orders {}",
        outcome.positions.len(),
        outcome.taker_orders().len()
    );
}

/// A failed poll must not consume the events it never delivered.
#[test]
fn a_failed_poll_does_not_consume_the_events_it_never_delivered() {
    let outcome = run(
        Scenario::new(vec![candidate("ph", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![Action::FailNextFillPoll, fill_resting("ph", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(position_ids(&outcome), vec!["ph".to_string()], "positions {:?}", position_ids(&outcome));
    assert_eq!(
        taker_sizes(&outcome, "ph", Venue::South),
        vec![30],
        "corrective orders after a poll failure are {:?}",
        taker_sizes(&outcome, "ph", Venue::South)
    );
}

/// A corrective order whose acknowledgement was lost is retried under its own
/// client reference after a restart, and the venue answers with the original
/// order rather than a second one.
#[test]
fn an_unacknowledged_correction_survives_a_restart_and_is_reattached() {
    let outcome = run(
        Scenario::new(vec![candidate("pi", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pi", Venue::North, 30)])
            .step(vec![Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.takers_for("pi", Venue::South).len(),
        1,
        "the venue holds {} corrective orders for one exposure after a lost acknowledgement and \
         a restart",
        outcome.takers_for("pi", Venue::South).len()
    );
    assert!(
        outcome
            .attempts
            .iter()
            .any(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_))),
        "the correction was never retried under its own client reference: {:?}",
        outcome
            .attempts
            .iter()
            .map(|attempt| &attempt.outcome)
            .collect::<Vec<_>>()
    );
}

/// A restart must not reopen an attribution window. The leg was tracked at t0;
/// a restart 800 seconds later must not admit a match stamped at t0 + 1000.
#[test]
fn a_restart_does_not_reopen_an_attribution_window() {
    let outcome = run(
        Scenario::new(vec![candidate("pj", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .with_cycle_seconds(0)
            .step(vec![])
            .step(vec![Action::AdvanceSeconds { seconds: 800 }])
            .step(vec![Action::Restart])
            .step(vec![
                Action::SetFillEventTimeOffset { seconds: 200 },
                fill_resting("pj", Venue::North, 30),
            ])
            .idle_steps(4),
    );
    assert!(
        outcome.positions.is_empty() && outcome.taker_orders().is_empty(),
        "a restart reopened the attribution window: an observation stamped 1000 seconds after the \
         order was tracked was counted, giving positions {} and corrective orders {}",
        outcome.positions.len(),
        outcome.taker_orders().len()
    );
}

/// The same block, naming the same orders in the other order, divides the same
/// way. Remainder placement is decided by the documented ordering, not by the
/// order the venue happened to build `covers` in.
#[test]
fn the_division_does_not_depend_on_the_order_covers_arrives_in() {
    fn division(covers: &[&str]) -> (Vec<Qty>, Vec<Qty>) {
        let outcome = run(
            Scenario::new(vec![
                candidate("pl", "nA", "tW", 0.45, 0.52, 10, 10),
                candidate("pm", "nB", "tW", 0.45, 0.52, 20, 20),
            ])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("tW"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pl", Venue::North, 10), fill_resting("pm", Venue::North, 20)])
            .step(vec![])
            .step(vec![block_takers(covers, Venue::South, 22, 0.55)])
            .idle_steps(6),
        );
        (
            taker_sizes(&outcome, "pl", Venue::South),
            taker_sizes(&outcome, "pm", Venue::South),
        )
    }
    assert_eq!(
        division(&["pl", "pm"]),
        division(&["pm", "pl"]),
        "the same block divided differently because `covers` arrived in a different order"
    );
}

/// Repeated cycles over a settled mandate are quiet.
#[test]
fn settled_work_stays_settled() {
    let outcome = run(
        Scenario::new(vec![candidate("pk", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("pk", Venue::North, 30), fill_resting("pk", Venue::South, 30)])
            .idle_steps(20),
    );
    assert_eq!(position_ids(&outcome), vec!["pk".to_string()], "positions {:?}", position_ids(&outcome));
    assert!(
        outcome.taker_orders().is_empty(),
        "a mandate whose both legs filled drew {:?} corrective orders",
        taker_sizes(&outcome, "pk", Venue::South)
    );
}
