//! Extra diagnostics, covering clauses the six named suites do not reach.
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

fn deduped(outcome: &Outcome) -> Vec<(String, Venue, OrderKind)> {
    outcome
        .attempts
        .iter()
        .filter(|attempt| matches!(attempt.outcome, SubmitOutcome::Deduplicated(_)))
        .map(|attempt| {
            (
                attempt.strategy_id.as_str().to_string(),
                attempt.venue,
                attempt.kind,
            )
        })
        .collect()
}

/// A block whose covered orders are all outside their attribution windows.
///
/// The division allocates nothing — the block is evidence about some other order
/// — but the venue closed the orders it named, so each leg's whole size is still
/// an open residual and has to be corrected.
#[test]
fn a_block_entirely_outside_every_window_allocates_nothing() {
    let outcome = run(
        Scenario::new(vec![
            candidate("za", "nA", "tZ", 0.45, 0.52, 10, 10),
            candidate("zb", "nB", "tZ", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tZ"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![fill_resting("za", Venue::North, 10), fill_resting("zb", Venue::North, 10)])
        .idle_steps(2)
        .step(vec![
            Action::AdvanceSeconds { seconds: 1_000 },
            block_takers(&["za", "zb"], Venue::South, 15, 0.55),
        ])
        .idle_steps(6),
    );
    assert_eq!(
        (taker_sizes(&outcome, "za", Venue::South), taker_sizes(&outcome, "zb", Venue::South)),
        (vec![10, 10], vec![10, 10]),
        "a block landing a thousand seconds after the orders it names were placed cannot be \
         attributed to them, and the venue closed them: each leg of 10 needs one follow-up \
         over the original corrective order, giving za {:?} and zb {:?}",
        taker_sizes(&outcome, "za", Venue::South),
        taker_sizes(&outcome, "zb", Venue::South)
    );
    assert!(
        outcome.positions.is_empty(),
        "a block outside every window attributes nothing, yet {:?} were recorded",
        position_ids(&outcome)
    );
}

/// The closure sequence, as the venue saw it: the resting order is cancelled
/// before any corrective order is sent, and the two are not in the same breath as
/// the decision that a correction was needed.
#[test]
fn a_corrective_order_follows_the_cancellation_of_the_resting_leg() {
    let outcome = run(
        Scenario::new(vec![candidate("zc", "nA", "tC2", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tC2"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("zc", Venue::North, 30)])
            .idle_steps(6),
    );
    let log = &outcome.venue_log;
    let cancel = log.iter().position(|event| matches!(event, VenueEvent::Cancelled { .. }));
    let corrective = log.iter().position(|event| {
        matches!(event, VenueEvent::Submitted { kind: OrderKind::ImmediateOrCancel, .. })
    });
    let (cancel, corrective) = match (cancel, corrective) {
        (Some(cancel), Some(corrective)) => (cancel, corrective),
        _ => panic!(
            "the engine neither cancelled the resting leg nor corrected it: {:?}",
            log
        ),
    };
    assert!(
        cancel < corrective,
        "the corrective order was sent before the resting leg was cancelled: {:?}",
        log
    );
    // The decision and the correction are a cycle apart, so a fill already in
    // flight has a cycle in which to land before another economic order is sent.
    let entry = log
        .iter()
        .position(|event| matches!(event, VenueEvent::Submitted { kind: OrderKind::RestingLimit, venue: Venue::South, .. }))
        .expect("the South leg was never placed");
    let cycles_between = |from: usize, to: usize| {
        log[from..to].iter().filter(|event| **event == VenueEvent::CycleEnd).count()
    };
    assert!(
        cycles_between(entry, cancel) >= 1,
        "the resting leg was cancelled in the very cycle it was placed: {:?}",
        log
    );
    assert_eq!(
        taker_sizes(&outcome, "zc", Venue::South),
        vec![30],
        "corrective orders {:?} were sent for one exposure of 30",
        taker_sizes(&outcome, "zc", Venue::South)
    );
}

/// The acknowledgement of an entry leg is lost. The order exists at the venue and
/// the engine must reattach to it rather than place the same exposure twice.
#[test]
fn an_entry_leg_whose_acknowledgement_was_lost_is_not_placed_twice() {
    let outcome = run(
        Scenario::new(vec![candidate("zd", "nA", "tD2", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tD2"), 0.54)
            .step(vec![Action::LoseNextAcknowledgementOn { venue: Venue::South }])
            .step(vec![fill_resting("zd", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.resting_for("zd", Venue::South).len(),
        1,
        "the venue holds {} resting orders for one South leg after its acknowledgement was lost",
        outcome.resting_for("zd", Venue::South).len()
    );
    // Every submission the engine made for that leg, however many it took, was
    // answered with the one order the venue already held.
    let south_refs: Vec<&ClientRef> = outcome
        .attempts
        .iter()
        .filter(|attempt| {
            attempt.strategy_id.as_str() == "zd"
                && attempt.venue == Venue::South
                && attempt.kind == OrderKind::RestingLimit
        })
        .map(|attempt| &attempt.client_ref)
        .collect();
    assert!(
        south_refs.windows(2).all(|pair| pair[0] == pair[1]),
        "the leg whose acknowledgement was lost was retried under more than one client \
         reference, which would place the same exposure twice: {:?}",
        south_refs
    );
    assert_eq!(position_ids(&outcome), vec!["zd".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A corrective order fills part of its size and is then closed by the venue in a
/// block. The gap it leaves behind is real and is corrected in full.
#[test]
fn a_finalized_corrective_order_leaves_its_whole_gap_open() {
    let outcome = run(
        Scenario::new(vec![candidate("ze", "nA", "tE2", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tE2"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("ze", Venue::North, 30)])
            .idle_steps(1)
            .step(vec![fill_taker("ze", Venue::South, 10)])
            .step(vec![block_takers(&["ze"], Venue::South, 5, 0.55)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "ze", Venue::South),
        vec![30, 15],
        "a corrective order of 30 that filled 10 and was then closed by a block of 5 leaves 15 \
         outstanding; the corrective orders sent were {:?}",
        taker_sizes(&outcome, "ze", Venue::South)
    );
}

/// Closure state survives a restart: the leg is still in closure, not resting
/// again, so the corrective order follows the cancellation rather than racing it.
#[test]
fn closure_state_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("zf", "nA", "tF2", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tF2"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("zf", Venue::North, 30)])
            .step(vec![Action::Restart])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "zf", Venue::South),
        vec![30],
        "after a restart mid-closure, corrective orders {:?} were sent for one exposure of 30",
        taker_sizes(&outcome, "zf", Venue::South)
    );
    assert_eq!(position_ids(&outcome), vec!["zf".to_string()], "positions {:?}", position_ids(&outcome));
}

/// A corrective order whose acknowledgement was lost is still owed after a
/// restart, and the retry reattaches to the order the venue already holds.
#[test]
fn a_lost_corrective_acknowledgement_survives_a_restart() {
    let outcome = run(
        Scenario::new(vec![candidate("zg", "nA", "tG2", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tG2"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("zg", Venue::North, 30)])
            .step(vec![Action::LoseNextAcknowledgement])
            .step(vec![Action::Restart])
            .idle_steps(4),
    );
    assert_eq!(
        taker_sizes(&outcome, "zg", Venue::South),
        vec![30],
        "the venue holds corrective orders {:?} for one exposure whose acknowledgement was lost",
        taker_sizes(&outcome, "zg", Venue::South)
    );
    assert!(
        deduped(&outcome).iter().any(|(_, _, kind)| *kind == OrderKind::ImmediateOrCancel),
        "the corrective order whose acknowledgement was lost was never retried under its own \
         client reference: {:?}",
        deduped(&outcome)
    );
}

/// The same fill delivered by the feed and then read from the live list is one
/// quantity, not two.
#[test]
fn the_feed_and_the_live_list_are_not_added_together() {
    let outcome = run(
        Scenario::new(vec![candidate("zh", "nA", "tH2", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tH2"), 0.60)
            .step(vec![])
            .step(vec![fill_resting("zh", Venue::North, 30), fill_resting("zh", Venue::South, 20)])
            .step(vec![Action::RedeliverAllFillEvents])
            .idle_steps(5),
    );
    assert_eq!(
        outcome.taker_qty_on_instrument("tH2"),
        10,
        "a South leg of 30 that filled 20 needed {} of correction; counting the feed and the live \
         list together would have sent nothing at all, and counting either twice would have sent \
         less",
        outcome.taker_qty_on_instrument("tH2")
    );
}

/// The worked example in `venue-semantics.md`: three eligible orders with
/// outstanding 10, 10 and 30 and a block of 37. The base shares are 7, 7 and 22,
/// the remainder of 1 goes to whichever sorts first, and the residual left across
/// the three legs is always the 13 the block did not cover.
#[test]
fn the_documented_worked_example_leaves_thirteen_outstanding() {
    for covering in [37, 25, 50, 13] {
        let outcome = run(
            Scenario::new(vec![
                candidate("zn", "nA", "tN2", 0.45, 0.52, 10, 10),
                candidate("zo", "nB", "tN2", 0.45, 0.52, 10, 10),
                candidate("zp", "nC", "tN2", 0.45, 0.52, 30, 30),
            ])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("nC"), 0.47)
            .ask(instrument("tN2"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![
                fill_resting("zn", Venue::North, 10),
                fill_resting("zo", Venue::North, 10),
                fill_resting("zp", Venue::North, 30),
            ])
            .idle_steps(1)
            .step(vec![block_takers(&["zn", "zo", "zp"], Venue::South, covering, 0.55)])
            .idle_steps(6),
        );
        // The first corrective order on each leg is that leg's whole size; what
        // follows it is what the block left uncovered.
        let follow_up: Qty = ["zn", "zo", "zp"]
            .iter()
            .map(|id| taker_sizes(&outcome, id, Venue::South).iter().skip(1).sum::<Qty>())
            .sum();
        let expected = (50 - covering).max(0);
        assert_eq!(
            follow_up, expected,
            "a block of {covering} over 50 outstanding left {follow_up} to correct; the block can \
             account for at most its own size, so {expected} should remain"
        );
    }
}

/// The order the venue happens to list a block's covered orders in must not
/// change what the block allocates.
#[test]
fn the_order_a_block_names_its_covered_orders_in_changes_nothing() {
    let build = |cover_order: Vec<&str>| {
        Scenario::new(vec![
            candidate("zk", "nA", "tK2", 0.45, 0.52, 8, 8),
            candidate("zl", "nB", "tK2", 0.45, 0.52, 12, 12),
            candidate("zm", "nC", "tK2", 0.45, 0.52, 20, 20),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("nC"), 0.47)
        .ask(instrument("tK2"), 0.54)
        .step(vec![Action::DisableTakerAutofill])
        .step(vec![
            fill_resting("zk", Venue::North, 8),
            fill_resting("zl", Venue::North, 12),
            fill_resting("zm", Venue::North, 20),
        ])
        .idle_steps(1)
        .step(vec![block_takers(&cover_order, Venue::South, 25, 0.55)])
        .idle_steps(6)
    };
    let one = run(build(vec!["zk", "zl", "zm"]));
    let other = run(build(vec!["zm", "zk", "zl"]));
    let sizes = |outcome: &Outcome| {
        (
            taker_sizes(outcome, "zk", Venue::South),
            taker_sizes(outcome, "zl", Venue::South),
            taker_sizes(outcome, "zm", Venue::South),
        )
    };
    assert_eq!(
        sizes(&one),
        sizes(&other),
        "the same block naming the same orders in a different order allocated differently: \
         {:?} against {:?}",
        sizes(&one),
        sizes(&other)
    );
}

/// A block over the resting legs of two mandates: each leg is closed by it and
/// corrected for what it did not fill, and nothing is cancelled at an order the
/// venue has already closed.
#[test]
fn a_block_over_entry_legs_closes_them_without_a_cancellation() {
    let outcome = run(
        Scenario::new(vec![
            candidate("zi", "nA", "tI2", 0.45, 0.52, 20, 20),
            candidate("zj", "nB", "tI2", 0.45, 0.52, 10, 10),
        ])
        .ask(instrument("nA"), 0.47)
        .ask(instrument("nB"), 0.47)
        .ask(instrument("tI2"), 0.54)
        .step(vec![])
        .step(vec![
            fill_resting("zi", Venue::North, 20),
            fill_resting("zj", Venue::North, 10),
            block_orders(
                vec![
                    OrderRef::Resting { strategy: strategy("zi"), venue: Venue::South },
                    OrderRef::Resting { strategy: strategy("zj"), venue: Venue::South },
                ],
                21,
                0.53,
                0,
            ),
        ])
        .idle_steps(8),
    );
    let sizes = (taker_sizes(&outcome, "zi", Venue::South), taker_sizes(&outcome, "zj", Venue::South));
    assert!(
        sizes.0.len() == 1 && sizes.1.len() == 1 && sizes.0[0] + sizes.1[0] == 9,
        "a block of 21 over 20 and 10 leaves 9 outstanding; it was corrected as {:?}",
        sizes
    );
    assert!(
        outcome.cancelled_order_ids.is_empty(),
        "{} cancellations were sent for orders the venue had already closed",
        outcome.cancelled_order_ids.len()
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["zi".to_string(), "zj".to_string()],
        "positions {:?}",
        position_ids(&outcome)
    );
}

/// A withdrawn order keeps the identifier the venue gave it, and the venue is
/// free to hand that identifier to an unrelated order once the window has
/// closed. Liveness read by identifier would let the withdrawn order — still in
/// the ledger, because its mandate never settled — report itself resting and
/// cancel the live leg of another mandate that now wears the identifier.
#[test]
fn a_withdrawn_order_does_not_claim_the_identifier_the_venue_recycled() {
    let outcome = run(
        Scenario::new(vec![candidate("ya", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("nB"), 0.47)
            .ask(instrument("tA"), 0.54)
            .ask(instrument("tB"), 0.54)
            .with_cycle_seconds(60)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("ya", Venue::North, 30)])
            .idle_steps(14)
            .step(vec![
                Action::AddCandidate { candidate: candidate("yb", "nB", "tB", 0.45, 0.52, 20, 20) },
                Action::RecycleClosedOrderId {
                    from: OrderRef::Resting { strategy: strategy("ya"), venue: Venue::South },
                },
            ])
            .step(vec![fill_resting("yb", Venue::South, 20)])
            .idle_steps(6),
    );
    assert_eq!(
        taker_sizes(&outcome, "yb", Venue::North),
        vec![20],
        "the live North leg of a mandate whose South leg filled was cancelled by an unrelated, \
         already-withdrawn order that used to wear its identifier; the corrective orders that \
         reached the venue were {:?}",
        taker_sizes(&outcome, "yb", Venue::North)
    );
    assert_eq!(
        outcome.cancelled_order_ids.len(),
        2,
        "each withdrawn order is cancelled once — the withdrawn leg of the first mandate, and \
         the live leg of the second that came to wear its identifier: {:?}",
        outcome.cancelled_order_ids
    );
}

/// A block is the venue's statement that the orders it names are closed. A block
/// that arrives while one of those orders is still unnamed — its acknowledgement
/// was lost — names an identifier this engine cannot yet read. That is not the
/// same as a block that names nothing of ours: the identifier arrives with the
/// retry, and the block still owes its division and its finality.
#[test]
fn a_block_naming_an_unnamed_order_is_read_once_the_order_is_named() {
    let outcome = run(
        Scenario::new(vec![candidate("ze", "nA", "tE", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tE"), 0.54)
            .step(vec![Action::DisableTakerAutofill])
            .step(vec![fill_resting("ze", Venue::North, 30)])
            .step(vec![Action::LoseNextAcknowledgement])
            .step(vec![block_takers(&["ze"], Venue::South, 30, 0.55)])
            .idle_steps(6),
    );
    assert_eq!(
        position_ids(&outcome),
        vec!["ze".to_string()],
        "the venue netted the corrective order in full and closed it, so the mandate is \
         complete; the positions recorded are {:?}",
        position_ids(&outcome)
    );
}


/// A block closes every order it names, including one outside its attribution
/// window that the division allocated nothing. A closed order can acquire nothing
/// more, so it is not outstanding however much of it was left unfilled.
#[test]
fn an_order_the_venue_closed_is_not_reported_outstanding() {
    let outcome = run(
        Scenario::new(vec![candidate("zh", "nA", "tH", 0.45, 0.52, 10, 10)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tH"), 0.54)
            .step(vec![])
            .step(vec![
                Action::AdvanceSeconds { seconds: 1_000 },
                block_orders(
                    vec![OrderRef::Resting { strategy: strategy("zh"), venue: Venue::South }],
                    4,
                    0.55,
                    0,
                ),
            ])
            .idle_steps(3),
    );
    let reported: Vec<(String, Venue)> = outcome
        .pending_orders
        .iter()
        .map(|order| (order.strategy_id.as_str().to_string(), order.venue))
        .collect();
    assert!(
        reported.iter().all(|(_, venue)| *venue == Venue::North),
        "the venue closed the South leg in a block a thousand seconds after it was placed; it \
         can acquire nothing more, yet it is still reported outstanding: {:?}",
        reported
    );
}
