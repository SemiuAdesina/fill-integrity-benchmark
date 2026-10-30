//! Diagnostics: how far an order has filled, from the feed and the live list.
//!
//! Each scenario drives the engine through its public surface against the
//! in-memory venue in `harness/` and reports what it observed. A failure names
//! the observable outcome that was wrong, in the terms of `/app/spec`. None of
//! this is graded.

mod harness;

use harness::*;

fn acquired(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Qty {
    outcome
        .venue_orders
        .iter()
        .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
        .map(|order| order.filled_qty)
        .sum()
}

/// One North fill, then the whole feed delivered again three times.
#[test]
fn a_redelivered_fill_event_adds_no_quantity() {
    let outcome = run(
        Scenario::new(vec![candidate("da", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![fill_resting("da", Venue::North, 30)])
            .step(vec![Action::RedeliverAllFillEvents])
            .step(vec![Action::RedeliverAllFillEvents])
            .step(vec![Action::RedeliverAllFillEvents])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        1,
        "after the feed delivered events it had already delivered, {} corrective orders \
         exist for one exposure",
        outcome.taker_orders().len()
    );
    assert!(
        acquired(&outcome, "da", Venue::South) <= 30,
        "the South leg of 30 acquired {} at the venue",
        acquired(&outcome, "da", Venue::South)
    );
    assert_eq!(outcome.positions.len(), 1, "{} positions for one mandate", outcome.positions.len());
}

/// The South leg fills 12 of 30. The feed reports it and the live list shows it;
/// they are two views of one figure.
#[test]
fn the_feed_and_the_live_list_report_one_quantity() {
    let outcome = run(
        Scenario::new(vec![candidate("db", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.60)
            .step(vec![])
            .step(vec![fill_resting("db", Venue::North, 30), fill_resting("db", Venue::South, 12)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_qty_on_instrument("tA"),
        18,
        "corrective quantity {} was sent for a leg of 30 that had filled 12",
        outcome.taker_qty_on_instrument("tA")
    );
    assert!(
        acquired(&outcome, "db", Venue::South) <= 30,
        "the South leg of 30 acquired {} at the venue",
        acquired(&outcome, "db", Venue::South)
    );
}

/// The live list alone reports partial progress while the feed is withheld.
#[test]
fn progress_reported_only_by_the_live_list_still_counts() {
    let outcome = run(
        Scenario::new(vec![candidate("dc", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.60)
            .step(vec![])
            .step(vec![fill_resting("dc", Venue::North, 30)])
            .step(vec![Action::WithholdFillEvents, fill_resting("dc", Venue::South, 21)])
            .idle_steps(2)
            .step(vec![Action::ReleaseFillEvents])
            .idle_steps(4),
    );
    assert_eq!(
        outcome.taker_qty_on_instrument("tA"),
        9,
        "corrective quantity {} was sent for a leg of 30 the live list showed filled to 21",
        outcome.taker_qty_on_instrument("tA")
    );
}

/// A match timestamped exactly at the instant the window closes. The engine
/// tracks the North leg at its first cycle; the fill arrives with a venue
/// timestamp ATTRIBUTION_WINDOW later.
#[test]
fn a_match_stamped_at_the_instant_the_window_closes_is_not_attributed() {
    let window = ATTRIBUTION_WINDOW.as_seconds();
    let outcome = run(
        Scenario::new(vec![candidate("dd", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![Action::SetFillEventTimeOffset { seconds: window - 10 }, fill_resting("dd", Venue::North, 30)])
            .idle_steps(6),
    );
    assert!(
        outcome.positions.is_empty() && outcome.taker_orders().is_empty(),
        "an observation stamped at window_start + ATTRIBUTION_WINDOW was counted: positions {}, \
         corrective orders {}",
        outcome.positions.len(),
        outcome.taker_orders().len()
    );
}

/// The same match one second earlier is the last instant inside the window.
#[test]
fn a_match_stamped_one_second_before_the_window_closes_is_attributed() {
    let window = ATTRIBUTION_WINDOW.as_seconds();
    let outcome = run(
        Scenario::new(vec![candidate("de", "nA", "tA", 0.45, 0.52, 30, 30)])
            .ask(instrument("nA"), 0.47)
            .ask(instrument("tA"), 0.54)
            .step(vec![])
            .step(vec![Action::SetFillEventTimeOffset { seconds: window - 11 }, fill_resting("de", Venue::North, 30)])
            .idle_steps(6),
    );
    assert_eq!(
        outcome.taker_orders().len(),
        1,
        "an observation stamped one second before window_start + ATTRIBUTION_WINDOW was not \
         counted: {} corrective orders",
        outcome.taker_orders().len()
    );
    assert_eq!(outcome.positions.len(), 1, "{} positions", outcome.positions.len());
}
