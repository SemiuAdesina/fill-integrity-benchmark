//! Extra diagnostics for contract points the six shipped suites do not reach.
//!
//! Same style as the shipped suites: drive the engine through its public surface
//! against the in-memory venue and assert on what the world observed.

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

/// Two corrective orders that share a creation instant because the clock is
/// held still, so only the venue tick that accepted them can order them.
#[test]
fn a_block_divides_by_venue_tick_when_windows_open_together() {
    let outcome = run(
        Scenario::new(vec![
            candidate("va", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("vb", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .with_cycle_seconds(0)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("va", Venue::North, 10)])
        .step(vec![])
        .step(vec![fill_resting("vb", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["vb", "va"], Venue::South, 11, 0.55)])
        .idle_steps(4),
    );
    assert_eq!(
        (taker_sizes(&outcome, "va", Venue::South), taker_sizes(&outcome, "vb", Venue::South)),
        (vec![10, 4], vec![10, 5]),
        "a block of 11 over two orders of 10 accepted in different ticks: the earlier tick takes \
         the remainder. va {:?}, vb {:?}",
        taker_sizes(&outcome, "va", Venue::South),
        taker_sizes(&outcome, "vb", Venue::South)
    );
}

/// The same, with a moving clock: the order whose window opened first is first.
#[test]
fn a_block_divides_by_window_start_first() {
    let outcome = run(
        Scenario::new(vec![
            candidate("wa", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("wb", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("wa", Venue::North, 10)])
        .step(vec![])
        .step(vec![fill_resting("wb", Venue::North, 10)])
        .step(vec![])
        .step(vec![block_takers(&["wb", "wa"], Venue::South, 11, 0.55)])
        .idle_steps(4),
    );
    assert_eq!(
        (taker_sizes(&outcome, "wa", Venue::South), taker_sizes(&outcome, "wb", Venue::South)),
        (vec![10, 4], vec![10, 5]),
        "a block of 11 over two orders of 10: the older window takes the remainder. wa {:?}, wb {:?}",
        taker_sizes(&outcome, "wa", Venue::South),
        taker_sizes(&outcome, "wb", Venue::South)
    );
}

/// The South leg reaches the venue but its acknowledgement is lost. It stays
/// tracked under the reference it was sent with: the venue must not end up
/// holding two legs for one mandate, and a resubmission has to carry the same
/// client reference so the venue's idempotency resolves it onto the original.
#[test]
fn an_unacknowledged_entry_leg_is_reattached_not_duplicated() {
    let outcome = run(
        Scenario::new(vec![candidate("la", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![fill_resting("la", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_for("la", Venue::South).len(),
        1,
        "the venue holds {} South legs for one mandate",
        outcome.resting_for("la", Venue::South).len()
    );
    let south_attempts: Vec<&SubmitAttempt> = outcome
        .attempts
        .iter()
        .filter(|attempt| attempt.venue == Venue::South && attempt.kind == OrderKind::RestingLimit)
        .collect();
    assert!(
        south_attempts
            .windows(2)
            .all(|pair| pair[0].client_ref == pair[1].client_ref),
        "a resubmission of the South leg did not carry the client reference it was sent under:          {:?}",
        south_attempts
            .iter()
            .map(|attempt| (attempt.client_ref.as_str(), &attempt.outcome))
            .collect::<Vec<_>>()
    );
    assert_eq!(position_ids(&outcome), vec!["la".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A leg marked for closure, then a restart, then the correction.
#[test]
fn closure_state_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("ra", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("ra", Venue::North, 30)])
            .step(vec![Action::Restart])
            .idle_steps(5),
    );
    assert_eq!(
        taker_sizes(&outcome, "ra", Venue::South),
        vec![30],
        "a restart between marking a leg for closure and correcting it produced {:?}",
        taker_sizes(&outcome, "ra", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["ra".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A corrective order whose acknowledgement was lost, then a restart, then the
/// retry. Only one corrective order may exist at the venue.
#[test]
fn an_unacknowledged_correction_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("na", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("na", Venue::North, 30)])
            .step(vec![Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .step(vec![fill_taker("na", Venue::South, 30)])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.takers_for("na", Venue::South).len(),
        1,
        "the venue holds {} corrective orders for one exposure",
        outcome.takers_for("na", Venue::South).len()
    );
    assert!(
        outcome
            .attempts
            .iter()
            .any(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_))),
        "the corrective order whose acknowledgement was lost was never retried under its own \
         client reference: {:?}",
        outcome.attempts.iter().map(|attempt| &attempt.outcome).collect::<Vec<_>>()
    );
    assert_eq!(position_ids(&outcome), vec!["na".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A block whose match instant falls outside every covered order's window is
/// evidence about some other order: it allocates nothing, and still closes them.
#[test]
fn a_block_outside_every_window_allocates_nothing_and_still_closes() {
    let outcome = run(
        Scenario::new(vec![
            candidate("oa", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("ob", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![])
        .step(vec![
            fill_resting("oa", Venue::North, 10),
            fill_resting("ob", Venue::North, 10),
            block_orders(
                vec![
                    OrderRef::Resting { strategy: strategy("oa"), venue: Venue::South },
                    OrderRef::Resting { strategy: strategy("ob"), venue: Venue::South },
                ],
                15,
                0.53,
                ATTRIBUTION_WINDOW.as_seconds() + 60,
            ),
        ])
        .idle_steps(8),
    );
    assert_eq!(
        (taker_sizes(&outcome, "oa", Venue::South), taker_sizes(&outcome, "ob", Venue::South)),
        (vec![10], vec![10]),
        "a block outside every window allocated nothing, so the whole of both legs had to be \
         covered: oa {:?}, ob {:?}",
        taker_sizes(&outcome, "oa", Venue::South),
        taker_sizes(&outcome, "ob", Venue::South)
    );
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        0,
        "{} cancellations were sent for orders the venue had already closed",
        outcome.cancelled_order_ids.len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["oa".to_string(), "ob".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A failed fill poll must not push the cursor past events that were never
/// delivered.
#[test]
fn a_failed_poll_does_not_consume_the_events_it_never_delivered() {
    let outcome = run(
        Scenario::new(vec![candidate("fa", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![Action::FailNextFillPoll, fill_resting("fa", Venue::North, 30)])
            .idle_steps(5),
    );
    assert_eq!(
        taker_sizes(&outcome, "fa", Venue::South),
        vec![30],
        "the fill the failed poll never delivered was counted as {:?} on the South leg",
        taker_sizes(&outcome, "fa", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["fa".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A leg that fills while it is being closed: the correction must not be sent
/// for quantity the desk already owns.
#[test]
fn a_fill_landing_while_the_leg_is_being_closed_shrinks_the_residual() {
    let outcome = run(
        Scenario::new(vec![candidate("ga", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.60)
            .step(vec![])
            .step(vec![fill_resting("ga", Venue::North, 30)])
            .step(vec![fill_resting("ga", Venue::South, 10)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "ga", Venue::South),
        vec![20],
        "corrective orders {:?} were sent for a leg of 30 that had filled 10",
        taker_sizes(&outcome, "ga", Venue::South)
    );
}

/// Two restarts and a block replay: the block is counted once and the residual
/// is corrected once.
#[test]
fn repeated_restarts_do_not_multiply_a_correction() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ha", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("hb", "nB", "tS", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("ha", Venue::North, 10), fill_resting("hb", Venue::North, 20)])
        .step(vec![])
        .step(vec![block_takers(&["ha", "hb"], Venue::South, 24, 0.55)])
        .step(vec![Action::Restart, Action::RedeliverAllBlocks])
        .step(vec![Action::Restart, Action::RedeliverAllBlocks])
        .idle_steps(5),
    );
    assert_eq!(
        (taker_sizes(&outcome, "ha", Venue::South), taker_sizes(&outcome, "hb", Venue::South)),
        (vec![10, 2], vec![20, 4]),
        "a block of 24 over 10 and 20, replayed across two restarts: ha {:?}, hb {:?}",
        taker_sizes(&outcome, "ha", Venue::South),
        taker_sizes(&outcome, "hb", Venue::South)
    );
}

/// A block bigger than everything it can be divided against: the excess belongs
/// to orders this engine does not track, and is dropped rather than invented.
#[test]
fn a_block_above_the_basis_drops_the_excess() {
    let outcome = run(
        Scenario::new(vec![
            candidate("ea", "nA", "tS", 0.45, 0.52, 10, 10),
            candidate("eb", "nB", "tS", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tS"), 0.54)
        .step(vec![])
        .step(vec![
            fill_resting("ea", Venue::North, 10),
            fill_resting("eb", Venue::North, 10),
            block_orders(
                vec![
                    OrderRef::Resting { strategy: strategy("ea"), venue: Venue::South },
                    OrderRef::Resting { strategy: strategy("eb"), venue: Venue::South },
                ],
                50,
                0.53,
                0,
            ),
        ])
        .idle_steps(8),
    );
    assert!(
        outcome.taker_orders().is_empty(),
        "a block of 50 over two legs of 10 completed them, yet {} corrective orders were sent",
        outcome.taker_orders().len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ea".to_string(), "eb".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}
