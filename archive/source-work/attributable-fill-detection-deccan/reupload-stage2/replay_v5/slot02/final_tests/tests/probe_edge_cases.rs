//! Extra probes beyond the six shipped suites. Not graded; used to hold the
//! engine to parts of the contract the shipped diagnostics do not reach.

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

/// The venue accepts the North leg but its acknowledgement never arrives. The
/// order exists and rests; the engine must recover it under the reference it
/// already chose rather than place the leg again.
#[test]
fn an_entry_leg_whose_acknowledgement_was_lost_is_not_placed_twice() {
    let outcome = run(
        Scenario::new(vec![candidate("pa", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::North }])
            .idle_steps(2)
            .step(vec![
                fill_resting("pa", Venue::North, 30),
                fill_resting("pa", Venue::South, 30),
            ])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.resting_for("pa", Venue::North).len(),
        1,
        "the venue holds {} North legs for one mandate after an acknowledgement was lost",
        outcome.resting_for("pa", Venue::North).len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pa".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
    assert!(
        outcome.taker_orders().is_empty(),
        "a corrective order was sent for a mandate that completed",
    );
}

/// The same, with a restart in between: the unacknowledged leg, its original
/// window and the reference its retry depends on all have to be durable.
#[test]
fn an_unacknowledged_entry_leg_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("pb", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::North }])
            .step(vec![Action::Restart])
            .idle_steps(2)
            .step(vec![
                fill_resting("pb", Venue::North, 30),
                fill_resting("pb", Venue::South, 30),
            ])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.resting_for("pb", Venue::North).len(),
        1,
        "the venue holds {} North legs for one mandate after a restart",
        outcome.resting_for("pb", Venue::North).len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pb".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A block whose match timestamp falls outside every covered order's window
/// allocates nothing, and still closes every order it names.
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
        .step(vec![
            fill_resting("pc", Venue::North, 10),
            fill_resting("pd", Venue::North, 20),
        ])
        .step(vec![])
        .step(vec![block_takers_at(
            &["pc", "pd"],
            Venue::South,
            24,
            0.55,
            ATTRIBUTION_WINDOW.as_seconds() * 2,
        )])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pc", Venue::South), taker_sizes(&outcome, "pd", Venue::South)),
        (vec![10, 10], vec![20, 20]),
        "a block outside every window allocates nothing but closes the orders it names, so the \
         whole of each leg is still owed: pc {:?}, pd {:?}",
        taker_sizes(&outcome, "pc", Venue::South),
        taker_sizes(&outcome, "pd", Venue::South)
    );
}

/// A block larger than the outstanding quantity it can be divided against: the
/// excess belongs to orders this engine does not track and is dropped.
#[test]
fn quantity_above_the_basis_is_dropped_rather_than_attributed() {
    let outcome = run(
        Scenario::new(vec![
            candidate("pe", "nA", "tE", 0.45, 0.52, 10, 10),
            candidate("pf", "nB", "tE", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tE"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![
            fill_resting("pe", Venue::North, 10),
            fill_resting("pf", Venue::North, 20),
        ])
        .step(vec![])
        .step(vec![block_takers(&["pe", "pf"], Venue::South, 100, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        2,
        "a block of 100 over outstanding 10 and 20 covers both legs in full; {} corrective \
         orders were sent",
        outcome.taker_orders().len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pe".to_string(), "pf".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// The division rule orders by the start of the attribution window before it
/// looks at the venue's identifier, and the identifier's lexicographic order
/// deliberately disagrees with it here.
#[test]
fn the_division_ordering_follows_the_window_before_the_identifier() {
    let outcome = run(
        Scenario::new(vec![
            candidate("pg", "nA", "tW", 0.45, 0.52, 10, 10),
            candidate("ph", "nB", "tW", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tW"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("pg", Venue::North, 10)])
        .step(vec![])
        .step(vec![fill_resting("ph", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["ph", "pg"], Venue::South, 11, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "pg", Venue::South), taker_sizes(&outcome, "ph", Venue::South)),
        (vec![10, 4], vec![10, 5]),
        "a block of 11 over two orders of 10 outstanding: the truncated unit goes to the order \
         whose window opened first. pg {:?}, ph {:?}",
        taker_sizes(&outcome, "pg", Venue::South),
        taker_sizes(&outcome, "ph", Venue::South)
    );
}

/// A failed cancellation leaves the resting order live, so the closure boundary
/// has not passed and no corrective order may go out yet.
#[test]
fn a_failed_cancellation_delays_the_correction_by_one_cycle() {
    let outcome = run(
        Scenario::new(vec![candidate("pi", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("pi", Venue::North, 30)])
            .step(vec![Action::FailNextCancel])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "pi", Venue::South),
        vec![30],
        "one cancellation failed before the closure boundary passed; corrective orders {:?}",
        taker_sizes(&outcome, "pi", Venue::South)
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pi".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A corrective order whose acknowledgement was lost, then a restart. The
/// reference its retry depends on has to be durable.
#[test]
fn an_unacknowledged_correction_survives_a_restart_and_is_retried_once() {
    let outcome = run(
        Scenario::new(vec![candidate("pj", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pj", Venue::North, 30)])
            .step(vec![Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .idle_steps(2)
            .step(vec![fill_taker("pj", Venue::South, 30)])
            .idle_steps(4),
    );
    assert_eq!(
        taker_sizes(&outcome, "pj", Venue::South),
        vec![30],
        "the venue holds {} corrective orders for one exposure after a lost acknowledgement and \
         a restart",
        taker_sizes(&outcome, "pj", Venue::South).len()
    );
    assert!(
        outcome
            .attempts
            .iter()
            .any(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_))),
        "the corrective order was never retried under its own client reference"
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pj".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A fill poll that fails must not consume the events it never delivered.
#[test]
fn a_failed_fill_poll_does_not_skip_the_events_it_never_delivered() {
    let outcome = run(
        Scenario::new(vec![candidate("pk", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.60)
            .step(vec![])
            .step(vec![fill_resting("pk", Venue::North, 30)])
            .step(vec![Action::FailNextFillPoll, fill_resting("pk", Venue::South, 12)])
            .idle_steps(8),
    );
    assert_eq!(
        outcome.taker_qty_on_instrument("tA"),
        18,
        "corrective quantity {} was sent for a leg of 30 that had filled 12 across a failed \
         poll",
        outcome.taker_qty_on_instrument("tA")
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pk".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A block that names one order only. The whole of the block is that order's,
/// up to what it had outstanding.
#[test]
fn a_block_naming_one_order_gives_it_the_total_up_to_its_outstanding() {
    let outcome = run(
        Scenario::new(vec![candidate("pl", "nA", "tL", 0.45, 0.52, 10, 10)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tL"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pl", Venue::North, 10)])
            .step(vec![])
            .step(vec![block_takers(&["pl"], Venue::South, 4, 0.55)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "pl", Venue::South),
        vec![10, 6],
        "a block of 4 against a correction of 10 leaves 6 outstanding: {:?}",
        taker_sizes(&outcome, "pl", Venue::South)
    );
    // The follow-up correction of 6 never fills, so the mandate stays one-legged.
    assert!(
        outcome.positions.is_empty(),
        "a leg of 10 that acquired 4 recorded a pairing"
    );
}

/// A match stamped at the very instant the window opens is inside it: the
/// window is half-open at the far end only.
#[test]
fn a_match_stamped_at_the_instant_the_window_opens_is_attributed() {
    let outcome = run(
        Scenario::new(vec![candidate("pn", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![
                Action::SetFillEventTimeOffset { seconds: -10 },
                fill_resting("pn", Venue::North, 30),
            ])
            .step(vec![Action::SetFillEventTimeOffset { seconds: 0 }])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        1,
        "an observation stamped at window_start was not counted: {} corrective orders",
        outcome.taker_orders().len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["pn".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// Two cycles of the same exposure: the residual is corrected exactly once even
/// though the correction never fills.
#[test]
fn an_unfilled_correction_blocks_a_second_one_for_the_same_exposure() {
    let outcome = run(
        Scenario::new(vec![candidate("pm", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("pm", Venue::North, 30)])
            .idle_steps(12)
            .step(vec![fill_taker("pm", Venue::South, 12)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "pm", Venue::South),
        vec![30],
        "one exposure of 30 drew corrective orders {:?}",
        taker_sizes(&outcome, "pm", Venue::South)
    );
    // 12 of the 30 filled and the other 18 is still working in that same order,
    // so the residual is zero: nothing more to send, and nothing paired yet.
    assert!(
        outcome.positions.is_empty(),
        "a leg of 30 that acquired 12 recorded a pairing"
    );
}
