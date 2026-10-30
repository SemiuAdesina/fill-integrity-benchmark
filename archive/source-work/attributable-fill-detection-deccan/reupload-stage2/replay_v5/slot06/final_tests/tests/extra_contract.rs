//! Extra diagnostics for corners the six shipped suites do not reach.

mod harness;

use harness::*;

fn taker_sizes(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Vec<Qty> {
    outcome
        .takers_for(strategy_id, venue)
        .into_iter()
        .map(|order| order.size)
        .collect()
}

/// Two corrective orders placed in different cycles, so their attribution
/// windows open at different instants. The block names them newest first.
#[test]
fn a_block_divides_by_window_start_before_identifier() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ea", "nA", "tE", 0.45, 0.52, 10, 10),
            candidate("eb", "nB", "tE", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tE"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ea", Venue::North, 10)])
        .step(vec![fill_resting("eb", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["eb", "ea"], Venue::South, 11, 0.55)])
        .idle_steps(6),
    );
    // ea's taker is a cycle older, so it sorts first and takes the remainder:
    // 11 over 10 and 10 gives 5 and 5 with one left, which goes to ea.
    assert_eq!(
        (taker_sizes(&outcome, "ea", Venue::South), taker_sizes(&outcome, "eb", Venue::South)),
        (vec![10, 4], vec![10, 5]),
        "ea {:?}, eb {:?}",
        taker_sizes(&outcome, "ea", Venue::South),
        taker_sizes(&outcome, "eb", Venue::South)
    );
}

/// A block whose match instant falls outside every covered order's window. It is
/// evidence about some other order, so it allocates nothing — but the venue has
/// still closed the orders it named.
#[test]
fn a_block_outside_every_window_allocates_nothing() {
    let window = ATTRIBUTION_WINDOW.as_seconds();
    let outcome = run(
        Scenario::new(vec![
            candidate("ec", "nA", "tF", 0.45, 0.52, 10, 10),
            candidate("ed", "nB", "tF", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tF"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ec", Venue::North, 10), fill_resting("ed", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers_at(&["ec", "ed"], Venue::South, 20, 0.55, window + 60)])
        .idle_steps(6),
    );
    // Nothing was allocated, so each leg still needs its full 10. The block
    // closed the first takers, so a fresh one is sent for each.
    assert_eq!(
        (taker_sizes(&outcome, "ec", Venue::South), taker_sizes(&outcome, "ed", Venue::South)),
        (vec![10, 10], vec![10, 10]),
        "ec {:?}, ed {:?}",
        taker_sizes(&outcome, "ec", Venue::South),
        taker_sizes(&outcome, "ed", Venue::South)
    );
}

/// A block larger than the outstanding quantity it can be divided against.
#[test]
fn a_block_above_the_basis_is_capped() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ee", "nA", "tG", 0.45, 0.52, 10, 10),
            candidate("ef", "nB", "tG", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tG"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ee", Venue::North, 10), fill_resting("ef", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["ee", "ef"], Venue::South, 40, 0.55)])
        .idle_steps(6),
    );
    // 20 outstanding can absorb 20 of the 40; the rest is not attributable.
    assert!(
        outcome.taker_orders().len() == 2,
        "a block that fills everything leaves nothing to correct: {:?}",
        taker_orders(&outcome)
    );
    assert_eq!(outcome.positions.len(), 2, "both mandates completed");
}

/// The second leg of a pair loses its acknowledgement. It exists at the venue
/// and fills there, and only the client reference ties that fill to the engine.
#[test]
fn an_unacknowledged_entry_leg_is_recovered_by_its_client_reference() {
    let outcome = run(
        Scenario::new(vec![candidate("eg", "nA", "tH", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tH"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![fill_resting("eg", Venue::North, 30), fill_resting("eg", Venue::South, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.venue_orders.len(),
        2,
        "one order per leg, the lost acknowledgement retried under its own reference: {} orders",
        outcome.venue_orders.len()
    );
    assert_eq!(outcome.positions.len(), 1, "both legs filled, so the mandate completed");
    assert!(outcome.taker_orders().is_empty(), "no exposure was left to correct");
}

/// A corrective order whose acknowledgement was lost, then a restart. The retry
/// has to go out under the same client reference, or the venue creates a second
/// order for the same exposure.
#[test]
fn an_unacknowledged_correction_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("eh", "nA", "tI", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tI"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("eh", Venue::North, 30), Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .idle_steps(3)
            .step(vec![fill_taker("eh", Venue::South, 30)])
            .idle_steps(3),
    );
    assert_eq!(
        outcome.takers_for("eh", Venue::South).len(),
        1,
        "{} corrective orders exist at the venue for one exposure",
        outcome.takers_for("eh", Venue::South).len()
    );
    assert!(outcome
        .attempts
        .iter()
        .any(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_))),
        "the retry did not go out under the original client reference"
    );
    assert_eq!(outcome.positions.len(), 1, "the mandate completed");
}

/// A cancellation that fails leaves the order live, so the correction has to
/// wait for a cycle in which the closure actually lands.
#[test]
fn a_failed_cancellation_defers_the_correction() {
    let outcome = run(
        Scenario::new(vec![candidate("ei", "nA", "tJ", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tJ"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ei", Venue::North, 30)])
            .step(vec![Action::FailNextCancel])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.takers_for("ei", Venue::South).len(),
        1,
        "{} corrective orders exist after a failed cancellation",
        outcome.takers_for("ei", Venue::South).len()
    );
    // The failed cancellation is not observable at the venue; the retry is. A
    // leg whose cancellation failed is still resting, so it is cancelled again
    // rather than corrected around.
    assert!(
        !outcome.cancelled_order_ids.is_empty(),
        "the cancellation was never retried after it failed"
    );
    assert_eq!(outcome.positions.len(), 1, "the mandate completed");
}

/// A block naming the order of a mandate that has already settled. The venue
/// recycled the identifier, so nothing about the settled mandate may answer for
/// an order that now wears it.
#[test]
fn a_block_naming_a_settled_mandates_identifier_answers_for_nothing() {
    let window = ATTRIBUTION_WINDOW.as_seconds();
    let outcome = run(
        Scenario::new(vec![candidate("ej", "nA", "tK", 0.45, 0.52, 20, 20)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tK"), 0.54)
            .with_cycle_seconds(60)
            .step(vec![])
            .step(vec![fill_resting("ej", Venue::North, 20), fill_resting("ej", Venue::South, 20)])
            .idle_steps(2)
            .step(vec![Action::AdvanceSeconds { seconds: window + 600 }])
            .step(vec![Action::RedeliverAllBlocks])
            .idle_steps(4),
    );
    assert_eq!(outcome.positions.len(), 1, "the mandate settled before the block");
    assert!(
        outcome.taker_orders().is_empty(),
        "a corrective order was sent for a settled mandate: {:?}",
        taker_orders(&outcome)
    );
}

/// An entry leg whose acknowledgement was lost, then a restart. The retry has to
/// go out under the same client reference or the venue creates a second order
/// for the same leg.
#[test]
fn an_unacknowledged_entry_leg_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("ek", "nA", "tL", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tL"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![Action::Restart])
            .idle_steps(4)
            .step(vec![fill_resting("ek", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.venue_orders.len(),
        3,
        "{} orders exist at the venue for a two-legged mandate with one correction",
        outcome.venue_orders.len()
    );
    assert_eq!(
        outcome.resting_for("ek", Venue::South).len(),
        1,
        "the South leg was submitted {} times at the venue",
        outcome.resting_for("ek", Venue::South).len()
    );
    assert_eq!(outcome.positions.len(), 1, "the mandate completed");
}

/// A block that nets a resting leg of one mandate together with the corrective
/// order of another, on one instrument.
#[test]
fn a_block_covering_a_resting_leg_and_a_correction_divides_per_order() {
    let outcome = run(
        Scenario::new(vec![
            candidate("el", "nA", "tM", 0.45, 0.52, 20, 20),
            candidate("em", "nB", "tM", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tM"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("em", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_orders(
            vec![
                OrderRef::Resting { strategy: strategy("el"), venue: Venue::South },
                OrderRef::Taker { strategy: strategy("em"), venue: Venue::South },
            ],
            18,
            0.55,
            0,
        )])
        .idle_steps(2)
        // el only becomes one-legged once the venue has already closed its
        // resting South leg in the block.
        .step(vec![fill_resting("el", Venue::North, 20)])
        .idle_steps(8),
    );
    // Outstanding 20 (el's resting leg) and 10 (em's correction): a block of 18
    // gives 12 and 6. el's resting leg is closed by the venue, so it needs no
    // cancellation and its residual of 8 is corrected directly.
    assert_eq!(
        (taker_sizes(&outcome, "el", Venue::South), taker_sizes(&outcome, "em", Venue::South)),
        (vec![8], vec![10, 4]),
        "el {:?}, em {:?}",
        taker_sizes(&outcome, "el", Venue::South),
        taker_sizes(&outcome, "em", Venue::South)
    );
    assert!(
        outcome
            .resting_for("el", Venue::South)
            .iter()
            .all(|order| !order.cancelled),
        "a cancellation was sent for a resting leg the venue had already closed in a block"
    );
}

/// A mandate that runs out of time. Its legs are cancelled and dropped, and it
/// is not re-entered cycle after cycle.
#[test]
fn a_mandate_past_its_cancel_lead_is_dropped_once() {
    let outcome = run(
        Scenario::new(vec![candidate("en", "nA", "tN", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tN"), 0.54)
            .step(vec![])
            .step(vec![Action::AdvanceSeconds { seconds: 86_200 }])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_orders().len(),
        2,
        "the mandate was re-entered after its legs were retired: {} resting orders",
        outcome.resting_orders().len()
    );
    assert!(outcome.positions.is_empty(), "a mandate that never filled recorded a pairing");
    assert!(outcome.pending_orders.is_empty(), "a retired leg is still reported as pending");
}

/// A correction that partly fills. The next cycle corrects only what is still
/// missing, counting the correction's own fill once and not twice.
#[test]
fn a_partly_filled_correction_is_followed_by_its_own_residual_only() {
    let outcome = run(
        Scenario::new(vec![candidate("eo", "nA", "tO", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tO"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("eo", Venue::North, 30)])
            .idle_steps(2)
            // The first correction of 30 fills 18, then the venue closes it in a
            // block that adds nothing. 12 remain.
            .step(vec![fill_taker("eo", Venue::South, 18)])
            .step(vec![block_takers(&["eo"], Venue::South, 0, 0.55)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "eo", Venue::South),
        vec![30, 12],
        "corrective orders {:?} for a leg of 30 whose first correction filled 18",
        taker_sizes(&outcome, "eo", Venue::South)
    );
}

/// A correction that fills in full completes the mandate, and no further
/// correction is sent.
#[test]
fn a_correction_that_fills_completes_the_mandate() {
    let outcome = run(
        Scenario::new(vec![candidate("ep", "nA", "tP", 0.45, 0.52, 25, 25)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tP"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("ep", Venue::North, 25)])
            .idle_steps(2)
            .step(vec![fill_taker("ep", Venue::South, 25)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "ep", Venue::South),
        vec![25],
        "corrective orders {:?} where one of 25 covered the exposure",
        taker_sizes(&outcome, "ep", Venue::South)
    );
    assert_eq!(outcome.positions.len(), 1, "the mandate completed once the correction filled");
}

/// A fill event stamped exactly at the instant the window opens. The window is
/// half-open at the start too, so that instant is inside it.
#[test]
fn a_fill_stamped_at_the_instant_the_window_opens_is_inside_it() {
    let outcome = run(
        Scenario::new(vec![candidate("eq", "nA", "tQ", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tQ"), 0.54)
            .step(vec![])
            // The legs were placed in the first cycle, at 1_000_000; this cycle's
            // clock is 1_000_010, so an offset of -10 lands the match instant on
            // the window's first instant.
            .step(vec![
                Action::SetFillEventTimeOffset { seconds: -10 },
                fill_resting("eq", Venue::North, 30),
            ])
            .step(vec![Action::SetFillEventTimeOffset { seconds: 0 }])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "eq", Venue::South),
        vec![30],
        "a fill stamped at the first instant of the window was not counted: the one-legged \
         mandate drew corrective orders {:?}",
        taker_sizes(&outcome, "eq", Venue::South)
    );
    assert_eq!(outcome.positions.len(), 1, "the mandate never completed");
}

/// A fill event stamped exactly at `window_start + ATTRIBUTION_WINDOW`. That
/// instant is outside the window, and the identifier it names is no longer this
/// order, so the fill is evidence about some other order.
#[test]
fn a_fill_stamped_at_the_closing_instant_of_the_window_is_outside_it() {
    let outcome = run(
        Scenario::new(vec![candidate("er", "nA", "tR", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tR"), 0.54)
            .step(vec![])
            // 1_000_010 + 890 = 1_000_900 = window_start + 900.
            .step(vec![
                Action::SetFillEventTimeOffset { seconds: 890 },
                fill_resting("er", Venue::North, 30),
            ])
            .idle_steps(6),
    );
    assert!(
        taker_sizes(&outcome, "er", Venue::South).is_empty(),
        "a fill stamped exactly at the closing instant of the window was counted, and the \
         mandate drew corrective orders {:?}",
        taker_sizes(&outcome, "er", Venue::South)
    );
    assert!(
        outcome.positions.is_empty(),
        "a fill from outside the window completed a mandate"
    );
}

/// A mandate that runs out of time after one leg filled and the other was
/// closed by the venue in a block. Neither is outstanding, so retirement owes
/// the venue no cancellation for either.
#[test]
fn retiring_a_mandate_cancels_nothing_the_venue_has_already_closed() {
    let outcome = run(
        Scenario::new(vec![candidate("es", "nA", "tS", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tS"), 0.54)
            .step(vec![])
            .step(vec![
                fill_resting("es", Venue::North, 30),
                block_orders(
                    vec![OrderRef::Resting { strategy: strategy("es"), venue: Venue::South }],
                    6,
                    0.53,
                    0,
                ),
            ])
            .step(vec![Action::AdvanceSeconds { seconds: 86_200 }])
            .idle_steps(6),
    );
    assert!(
        outcome.cancelled_order_ids.is_empty(),
        "a mandate that ran out of time drew cancellations {:?}, but the venue had already \
         closed both of its legs",
        outcome.cancelled_order_ids
    );
}

/// A leg that has filled in full is not outstanding at the venue, so it is not
/// one of the orders the engine still reports as pending.
#[test]
fn a_leg_that_has_filled_in_full_is_not_reported_as_pending() {
    let outcome = run(
        Scenario::new(vec![candidate("et", "nA", "tT", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tT"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("et", Venue::North, 30)]),
    );
    let pending: Vec<Venue> = outcome
        .pending_orders
        .iter()
        .map(|order| order.venue)
        .collect();
    assert_eq!(
        pending,
        vec![Venue::South],
        "the North leg filled in full and the venue no longer holds it, yet the engine reports \
         pending orders on {:?}",
        pending
    );
}

/// A leg whose acknowledgement was lost twice, so that its identity comes back
/// from the live order list rather than from an acceptance. The tick that
/// accepted it comes back with it.
#[test]
fn an_identity_recovered_from_the_live_list_carries_its_venue_tick() {
    let outcome = run(
        Scenario::new(vec![candidate("eu", "nA", "tU", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tU"), 0.54)
            .step(vec![
                Action::LoseNextAcknowledgementOn { venue: Venue::South },
                Action::LoseNextAcknowledgementOn { venue: Venue::South },
            ])
            .idle_steps(4),
    );
    let south: Vec<u64> = outcome
        .pending_orders
        .iter()
        .filter(|order| order.venue == Venue::South)
        .map(|order| order.venue_seq)
        .collect();
    assert_eq!(
        south,
        vec![1],
        "a South leg recovered from the live order list reports venue tick {:?}, so it would be \
         ordered as if it had arrived before every other order",
        south
    );
}

fn taker_orders(outcome: &Outcome) -> Vec<(String, Venue, Qty)> {
    outcome
        .taker_orders()
        .iter()
        .map(|order| (order.strategy_id.as_str().to_string(), order.venue, order.size))
        .collect()
}
