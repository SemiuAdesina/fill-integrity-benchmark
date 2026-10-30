//! Diagnostics: mandates are independent of one another.
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

/// Two mandates whose South legs rest on one instrument. Custody inventory on
/// that instrument rises when either fills.
#[test]
fn inventory_on_a_shared_instrument_completes_nothing_by_itself() {
    let outcome = run(
        Scenario::new(vec![
            candidate("xa", "nA", "tS", 0.45, 0.52, 20, 20),
            candidate("xb", "nB", "tS", 0.44, 0.53, 20, 20),
        ])
        .inventory(instrument("tS"), 45)
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![])
        .step(vec![fill_resting("xa", Venue::North, 20)])
        .idle_steps(6),
    );
    assert!(
        outcome.position("xb").is_none(),
        "a mandate none of whose orders filled recorded a pairing"
    );
    assert!(
        outcome.position("xa").is_some(),
        "the mandate whose North leg filled and whose South leg was corrected never completed"
    );
    assert_eq!(
        taker_sizes(&outcome, "xa", Venue::South),
        vec![20],
        "corrective orders for the one-legged mandate are {:?}",
        taker_sizes(&outcome, "xa", Venue::South)
    );
    assert!(
        outcome.takers_for("xb", Venue::South).is_empty() && outcome.takers_for("xb", Venue::North).is_empty(),
        "a corrective order was sent for a mandate none of whose orders filled"
    );
}

/// Two mandates on disjoint instruments, each one-legged three cycles apart.
#[test]
fn each_one_legged_mandate_gets_its_own_corrective_order() {
    let outcome = run(
        Scenario::new(vec![
            candidate("xc", "n1", "t1", 0.45, 0.52, 30, 30),
            candidate("xd", "n2", "t2", 0.44, 0.53, 30, 30),
        ])
        .ask(instrument("n1"), 0.47)
        .ask(instrument("t1"), 0.54)
        .ask(instrument("n2"), 0.46)
        .ask(instrument("t2"), 0.55)
        .step(vec![])
        .step(vec![fill_resting("xc", Venue::North, 30)])
        .idle_steps(3)
        .step(vec![fill_resting("xd", Venue::South, 30)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "xc", Venue::South), taker_sizes(&outcome, "xd", Venue::North)),
        (vec![30], vec![30]),
        "corrective orders per mandate: xc South {:?}, xd North {:?}",
        taker_sizes(&outcome, "xc", Venue::South),
        taker_sizes(&outcome, "xd", Venue::North)
    );
    assert_eq!(outcome.positions.len(), 2, "{} positions for two mandates", outcome.positions.len());
}

/// The venue nets the corrective orders of two mandates on one instrument into
/// one block of 21 over 10 and 20.
#[test]
fn a_block_over_two_mandates_orders_is_divided_per_order() {
    let outcome = run(
        Scenario::new(vec![
            candidate("xe", "nA", "tG", 0.45, 0.52, 10, 10),
            candidate("xf", "nB", "tG", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tG"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("xe", Venue::North, 10), fill_resting("xf", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers(&["xf", "xe"], Venue::South, 21, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "xe", Venue::South), taker_sizes(&outcome, "xf", Venue::South)),
        (vec![10, 3], vec![20, 6]),
        "after a block of 21 over corrective orders of 10 and 20 the corrective orders are \
         xe {:?}, xf {:?}",
        taker_sizes(&outcome, "xe", Venue::South),
        taker_sizes(&outcome, "xf", Venue::South)
    );
}
