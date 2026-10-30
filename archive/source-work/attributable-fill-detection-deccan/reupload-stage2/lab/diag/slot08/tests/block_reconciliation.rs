//! Diagnostics: one venue match netted across several of the engine's orders.
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

/// Three corrective orders of 8, 12 and 20 rest unfilled on one instrument and
/// are accepted in one venue tick. A block of 25 nets them.
#[test]
fn a_block_total_is_divided_over_outstanding_quantity_in_the_documented_order() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ba", "nA", "tS", 0.45, 0.52, 8, 8),
            candidate("bb", "nB", "tS", 0.45, 0.52, 12, 12),
            candidate("bc", "nC", "tS", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("nC"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![
            fill_resting("ba", Venue::North, 8),
            fill_resting("bb", Venue::North, 12),
            fill_resting("bc", Venue::North, 20),
        ])
        .step(vec![])
        .step(vec![block_takers(&["bc", "ba", "bb"], Venue::South, 25, 0.55)])
        .idle_steps(6),
    );
    let sizes = (
        taker_sizes(&outcome, "ba", Venue::South),
        taker_sizes(&outcome, "bb", Venue::South),
        taker_sizes(&outcome, "bc", Venue::South),
    );
    assert_eq!(
        sizes,
        (vec![8, 3], vec![12, 5], vec![20, 7]),
        "after a block of 25 over outstanding 8, 12 and 20 the corrective orders sent were \
         {:?}; the division rule leaves residuals of 3, 5 and 7",
        sizes
    );
}

/// The same block, delivered by the feed three times under one `block_ref`.
#[test]
fn a_redelivered_block_is_counted_once() {
    let outcome = run(
        Scenario::new(vec![
            candidate("bd", "nA", "tR", 0.45, 0.52, 10, 10),
            candidate("be", "nB", "tR", 0.45, 0.52, 30, 30),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tR"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("bd", Venue::North, 10), fill_resting("be", Venue::North, 30)])
        .step(vec![])
        .step(vec![block_takers(&["bd", "be"], Venue::South, 30, 0.55)])
        .step(vec![Action::RedeliverAllBlocks])
        .step(vec![Action::RedeliverAllBlocks])
        .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        4,
        "a block of 30 over 10 and 30 was delivered three times; {} corrective orders exist \
         where one division and one follow-up per order would give 4",
        outcome.taker_orders().len()
    );
    assert!(
        outcome.positions.is_empty(),
        "a block of 30 cannot complete legs totalling 40, yet positions were recorded"
    );
}

/// Two corrective orders of 10 and 30 are partly filled to 9 and 6, then a
/// block of 12 names both. Its division gives one of them nothing at all.
#[test]
fn every_order_a_block_names_is_closed_by_it_even_one_allocated_nothing() {
    let outcome = run(
        Scenario::new(vec![
            candidate("bf", "nA", "tQ", 0.45, 0.52, 10, 10),
            candidate("bg", "nB", "tQ", 0.45, 0.52, 30, 30),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tQ"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("bf", Venue::North, 10), fill_resting("bg", Venue::North, 30)])
        .step(vec![])
        .step(vec![fill_taker("bf", Venue::South, 9), fill_taker("bg", Venue::South, 6)])
        .step(vec![block_takers(&["bf", "bg"], Venue::South, 12, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "bf", Venue::South),
        vec![10, 1],
        "the block named the order with 1 outstanding and allocated it nothing; the venue \
         closed it, but the corrective orders for that leg are {:?}",
        taker_sizes(&outcome, "bf", Venue::South)
    );
    assert_eq!(
        taker_sizes(&outcome, "bg", Venue::South),
        vec![30, 12],
        "corrective orders for the leg the block allocated 12 to are {:?}",
        taker_sizes(&outcome, "bg", Venue::South)
    );
}
