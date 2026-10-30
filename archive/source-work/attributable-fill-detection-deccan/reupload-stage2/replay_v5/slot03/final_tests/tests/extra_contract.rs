//! Extra diagnostics for contract areas the six shipped suites do not reach.
//!
//! Not graded; a working set for checking a reading of `/app/spec`.

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

/// A block whose match instant falls outside every covered order's window
/// allocates nothing, and still closes them all.
#[test]
fn a_block_outside_every_window_allocates_nothing_and_closes_everything() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ea", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("eb", "nB", "tS", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ea", Venue::North, 10), fill_resting("eb", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers_at(&["ea", "eb"], Venue::South, 25, 0.55, 5_000)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "ea", Venue::South), taker_sizes(&outcome, "eb", Venue::South)),
        (vec![10, 10], vec![20, 20]),
        "a block stamped outside both windows allocated nothing, so each leg is still short its \
         full size: ea {:?}, eb {:?}",
        taker_sizes(&outcome, "ea", Venue::South),
        taker_sizes(&outcome, "eb", Venue::South)
    );
    // Taker autofill is off for the whole run, so the replacements rest unfilled
    // and neither mandate completes. What matters is that the out-of-window block
    // contributed no quantity while still closing every order it named.
    assert!(
        outcome.positions.is_empty(),
        "a block that allocated nothing completed a mandate: {:?}",
        position_ids(&outcome)
    );
}

/// A block larger than the outstanding quantity it can be divided against.
#[test]
fn quantity_above_the_basis_is_dropped() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ec", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("ed", "nB", "tS", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ec", Venue::North, 10), fill_resting("ed", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers(&["ec", "ed"], Venue::South, 500, 0.55)])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "ec", Venue::South), taker_sizes(&outcome, "ed", Venue::South)),
        (vec![10], vec![20]),
        "a block of 500 over outstanding 10 and 20 fills both outright and leaves no residual: \
         ec {:?}, ed {:?}",
        taker_sizes(&outcome, "ec", Venue::South),
        taker_sizes(&outcome, "ed", Venue::South)
    );
    assert_eq!(position_ids(&outcome).len(), 2, "positions {:?}", position_ids(&outcome));
}

/// A withdrawal the engine cannot confirm is never corrected against, however
/// many cycles pass.
#[test]
fn an_unconfirmed_leg_is_still_not_corrected_on_later_cycles() {
    let outcome = run(
        Scenario::new(vec![candidate("ee", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ee", Venue::North, 30)])
            .step(vec![Action::WithholdFillEvents, fill_resting("ee", Venue::South, 30)])
            .idle_steps(10),
    );
    assert!(
        outcome.takers_on_instrument("tA").is_empty(),
        "a corrective order was sent for a leg that had left the live list without the engine \
         cancelling it: {:?}",
        taker_sizes(&outcome, "ee", Venue::South)
    );
}

/// A cancellation the venue refuses leaves the resting order live, so the
/// corrective order must not go out that cycle.
#[test]
fn a_refused_cancellation_defers_the_corrective_order() {
    let outcome = run(
        Scenario::new(vec![candidate("ef", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ef", Venue::North, 30)])
            .step(vec![Action::FailNextCancel])
            .idle_steps(5),
    );
    assert_eq!(
        taker_sizes(&outcome, "ef", Venue::South),
        vec![30],
        "one cancellation failed, leaving the resting order live; corrective orders are {:?}",
        taker_sizes(&outcome, "ef", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["ef".to_string()], "positions {:?}", position_ids(&outcome));
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        1,
        "the resting order was cancelled {:?} times for one exposure",
        outcome.cancelled_order_ids.len()
    );
}

/// The process restarts in the cycle between the cancellation and the corrective
/// order. The closure state has to survive.
#[test]
fn a_restart_between_the_cancellation_and_the_correction_closes_the_gap_once() {
    let outcome = run(
        Scenario::new(vec![candidate("eg", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("eg", Venue::North, 30)])
            .step(vec![Action::DisableTakerAutofill, Action::Restart])
            .idle_steps(2)
            .step(vec![fill_taker("eg", Venue::South, 30)])
            .idle_steps(3),
    );
    assert_eq!(
        taker_sizes(&outcome, "eg", Venue::South),
        vec![30],
        "corrective orders after a restart between cancellation and correction: {:?}",
        taker_sizes(&outcome, "eg", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["eg".to_string()], "positions {:?}", position_ids(&outcome));
}

/// The venue closes a resting leg in a block without filling any of it, while the
/// other leg is still resting and never fills.
#[test]
fn a_leg_closed_short_by_the_venue_is_corrected_without_waiting_for_the_other() {
    let outcome = run(
        Scenario::new(vec![candidate("eh", "nA", "tA", 0.45, 0.52, 10, 10)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![block_orders(
                vec![OrderRef::Resting { strategy: strategy("eh"), venue: Venue::South }],
                0,
                0.53,
                0,
            )])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "eh", Venue::South),
        vec![10],
        "the venue closed the South leg in a block that filled none of it; corrective orders {:?}",
        taker_sizes(&outcome, "eh", Venue::South)
    );
    // Only the North leg is still resting, so only that one is cancelled.
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        1,
        "{} cancellations were sent where only the North leg was still resting",
        outcome.cancelled_order_ids.len()
    );
}

/// A block naming an order that had already filled. It allocates nothing and
/// closes an order that needed no closing.
#[test]
fn a_block_naming_a_fully_filled_order_changes_nothing() {
    let outcome = run(
        Scenario::new(vec![candidate("ei", "nA", "tA", 0.45, 0.52, 10, 10)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ei", Venue::North, 10), fill_resting("ei", Venue::South, 10)])
            .idle_steps(6),
    );
    assert_eq!(position_ids(&outcome), vec!["ei".to_string()], "positions {:?}", position_ids(&outcome));
    assert!(outcome.taker_orders().is_empty(), "a completed mandate drew a corrective order");
}

/// Two mandates, one shared instrument, one block, and the feed withheld at the
/// moment the block is reported.
#[test]
fn a_block_absorbed_while_the_feed_is_withheld_is_absorbed_when_it_returns() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ej", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("ek", "nB", "tS", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ej", Venue::North, 10), fill_resting("ek", Venue::North, 20)])
        .step(vec![])
        .step(vec![
            Action::WithholdFillEvents,
            block_takers(&["ej", "ek"], Venue::South, 21, 0.55),
        ])
        .idle_steps(2)
        .step(vec![Action::ReleaseFillEvents])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "ej", Venue::South), taker_sizes(&outcome, "ek", Venue::South)),
        (vec![10, 3], vec![20, 6]),
        "a block of 21 over 10 and 20 leaves residuals of 3 and 6: ej {:?}, ek {:?}",
        taker_sizes(&outcome, "ej", Venue::South),
        taker_sizes(&outcome, "ek", Venue::South)
    );
}

/// Two corrective orders whose windows opened in different cycles. The remainder
/// of the division goes to the earlier window, not to the larger order.
#[test]
fn the_block_remainder_follows_the_window_order_not_the_larger_order() {
    let outcome = run(
        Scenario::new(vec![candidate("el", "nA", "tS", 0.45, 0.52, 10, 10)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("tS"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("el", Venue::North, 10)])
            .step(vec![Action::AddCandidate {
                candidate: candidate("em", "nB", "tS", 0.45, 0.52, 30, 30),
            }])
            .step(vec![fill_resting("em", Venue::North, 30)])
            .idle_steps(3)
            .step(vec![block_takers(&["el", "em"], Venue::South, 37, 0.55)])
            .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "el", Venue::South), taker_sizes(&outcome, "em", Venue::South)),
        (vec![10], vec![30, 3]),
        "a block of 37 over outstanding 10 and 30 gives base shares 9 and 27 and one remainder \
         unit to the earlier window, so that leg completes and only the later one is short: \
         el {:?}, em {:?}",
        taker_sizes(&outcome, "el", Venue::South),
        taker_sizes(&outcome, "em", Venue::South)
    );
}

/// An entry leg whose acknowledgement is lost. The order exists at the venue, and
/// only the client reference ties it back to the engine.
#[test]
fn an_entry_leg_whose_acknowledgement_was_lost_is_retried_under_its_own_reference() {
    let outcome = run(
        Scenario::new(vec![candidate("en", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![fill_resting("en", Venue::North, 30), fill_resting("en", Venue::South, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_for("en", Venue::South).len(),
        1,
        "{} resting orders exist at the venue for one South leg after its acknowledgement was lost",
        outcome.resting_for("en", Venue::South).len()
    );
    assert!(
        outcome.taker_orders().is_empty(),
        "both legs filled at the venue, yet corrective orders were sent: {:?}",
        taker_sizes(&outcome, "en", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["en".to_string()], "positions {:?}", position_ids(&outcome));
}

/// Every cycle's snapshot is written even when a port fails, so the cursor never
/// falls behind the evidence the cycle already absorbed.
#[test]
fn a_failed_port_does_not_cost_the_cycle_its_durable_state() {
    let outcome = run(
        Scenario::new(vec![candidate("eo", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("eo", Venue::North, 30)])
            .step(vec![Action::FailNextInventoryRead])
            .step(vec![Action::Restart, Action::RedeliverAllFillEvents])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        1,
        "{} corrective orders exist for one exposure across a failed port and a restart",
        outcome.taker_orders().len()
    );
    assert_eq!(position_ids(&outcome), vec!["eo".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A poll failure must not advance the cursor past events that were never
/// delivered.
#[test]
fn a_failed_poll_leaves_the_cursor_where_it_was() {
    let outcome = run(
        Scenario::new(vec![candidate("ep", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![Action::FailNextFillPoll, fill_resting("ep", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_qty_on_instrument("tA"),
        30,
        "corrective quantity {} for a North leg of 30 whose fill event was delayed by a poll \
         failure",
        outcome.taker_qty_on_instrument("tA")
    );
    assert_eq!(position_ids(&outcome), vec!["ep".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A corrective order whose acknowledgement was lost, then a restart before the
/// venue can be asked about it again.
#[test]
fn an_unacknowledged_correction_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("eq", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("eq", Venue::North, 30), Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .idle_steps(2)
            .step(vec![fill_taker("eq", Venue::South, 30)])
            .idle_steps(3),
    );
    assert_eq!(
        outcome.takers_for("eq", Venue::South).len(),
        1,
        "the venue holds {} corrective orders for one exposure after a lost acknowledgement and \
         a restart",
        outcome.takers_for("eq", Venue::South).len()
    );
    assert!(
        outcome
            .attempts
            .iter()
            .any(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_))),
        "the unacknowledged correction was never retried under its own client reference"
    );
    assert_eq!(position_ids(&outcome), vec!["eq".to_string()], "positions {:?}", position_ids(&outcome));
}

/// One venue match netting a still-resting leg of one mandate together with the
/// corrective order of another. Both are closed by it; neither acquires more than
/// its size.
#[test]
fn a_block_over_a_resting_leg_and_a_correction_of_another_mandate() {
    let outcome = run(
        Scenario::new(vec![
            candidate("er", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("es", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("es", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_orders(
            vec![
                OrderRef::Resting { strategy: strategy("er"), venue: Venue::South },
                OrderRef::Taker { strategy: strategy("es"), venue: Venue::South },
            ],
            12,
            0.53,
            0,
        )])
        .idle_steps(2)
        .step(vec![Action::EnableTakerAutofill])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "er", Venue::South), taker_sizes(&outcome, "es", Venue::South)),
        (vec![4], vec![10, 4]),
        "the block gave 6 each over outstanding 10 and 10, leaving residuals of 4 and 4: \
         er {:?}, es {:?}",
        taker_sizes(&outcome, "er", Venue::South),
        taker_sizes(&outcome, "es", Venue::South)
    );
    for strategy_id in ["er", "es"] {
        for venue in [Venue::North, Venue::South] {
            let acquired: Qty = outcome
                .venue_orders
                .iter()
                .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
                .map(|order| order.filled_qty)
                .sum();
            assert!(
                acquired <= 10,
                "{strategy_id} acquired {acquired} on {venue} against a leg of 10"
            );
        }
    }
}
