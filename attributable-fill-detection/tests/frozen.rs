//! Regression suite: behaviour the task does *not* ask to change.
//!
//! The fix touches fill attribution. This file guards everything adjacent to it,
//! so a candidate cannot reach a green graded suite by loosening a comparison,
//! shifting the exit window, or rewriting position arithmetic. These are pure
//! unit assertions on the frozen contract in `/app/spec/frozen-contract.md`; no
//! engine cycle is driven here.
//!
//! Covered: the cancel lead, the exit window and its inclusive bounds, expiry,
//! position cost arithmetic, the quantity epsilon comparisons (`qty_eq` and
//! `qty_at_least`, including the exact-boundary case), timestamp and duration
//! arithmetic, `Cursor` defaults, and `EngineConfig` defaults.
//!
//! A failure here means the candidate changed something outside the task's scope.

use pairing_engine::types::ids::{qty_at_least, qty_eq, Venue, QTY_EPSILON};
use pairing_engine::types::strategy::StrategyCandidate;
use pairing_engine::types::pending_order::{PendingOrder, CANCEL_LEAD};
use pairing_engine::types::position::{Position, EXIT_WINDOW_END_SECONDS, EXIT_WINDOW_START_SECONDS};
use pairing_engine::types::snapshot::Cursor;
use pairing_engine::types::time::{Duration, Timestamp};
use pairing_engine::EngineConfig;

fn candidate(expiry_seconds: i64) -> StrategyCandidate {
    StrategyCandidate::new(
        "cond".into(),
        "north-instrument".into(),
        "south-instrument".into(),
        0.46,
        0.5,
        80.0,
        40.0,
        Timestamp::from_seconds(expiry_seconds),
    )
}

fn leg(venue: Venue, expiry_seconds: i64) -> PendingOrder {
    PendingOrder::new(
        "cond".into(),
        "north-instrument".into(),
        venue,
        "order-1".into(),
        "pex-1".into(),
        0.46,
        25.0,
        Timestamp::from_seconds(expiry_seconds),
        Timestamp::from_seconds(0),
    )
}

fn position(expiry_seconds: i64) -> Position {
    Position::new(
        "cond".into(),
        30.0,
        20.0,
        0.46,
        0.5,
        Timestamp::from_seconds(expiry_seconds),
        Timestamp::from_seconds(0),
    )
}

#[test]
fn cancel_lead_is_five_minutes() {
    assert_eq!(CANCEL_LEAD.as_seconds(), 300);
}

#[test]
fn pending_order_cancels_only_inside_the_cancel_lead() {
    let order = leg(Venue::North, 1_000);
    assert!(!order.should_cancel(Timestamp::from_seconds(699)));
    assert!(order.should_cancel(Timestamp::from_seconds(701)));
    assert!(order.should_cancel(Timestamp::from_seconds(1_000)));
    assert!(order.should_cancel(Timestamp::from_seconds(1_400)));
}

#[test]
fn pending_order_reports_remaining_seconds() {
    let order = leg(Venue::South, 900);
    assert_eq!(
        order.seconds_until_expiry(Timestamp::from_seconds(300)),
        600
    );
    assert_eq!(
        order.seconds_until_expiry(Timestamp::from_seconds(960)),
        -60
    );
}

#[test]
fn exit_window_bounds_are_unchanged() {
    assert_eq!(EXIT_WINDOW_START_SECONDS, 60);
    assert_eq!(EXIT_WINDOW_END_SECONDS, 120);
}

#[test]
fn position_exit_window_is_inclusive_at_both_ends() {
    let held = position(1_000);
    assert!(!held.should_exit_now(Timestamp::from_seconds(879)));
    assert!(held.should_exit_now(Timestamp::from_seconds(880)));
    assert!(held.should_exit_now(Timestamp::from_seconds(920)));
    assert!(held.should_exit_now(Timestamp::from_seconds(940)));
    assert!(!held.should_exit_now(Timestamp::from_seconds(941)));
}

#[test]
fn position_flags_the_final_minute_and_expiry() {
    let held = position(1_000);
    assert!(!held.is_too_close_to_expiry(Timestamp::from_seconds(940)));
    assert!(held.is_too_close_to_expiry(Timestamp::from_seconds(941)));
    assert!(!held.is_expired(Timestamp::from_seconds(999)));
    assert!(held.is_expired(Timestamp::from_seconds(1_000)));
    assert!(held.is_expired(Timestamp::from_seconds(1_001)));
}

#[test]
fn position_costs_are_derived_from_entry_prices_and_sizes() {
    let held = position(1_000);
    assert!((held.pair_cost - 0.96).abs() < 1e-9);
    assert!((held.total_notional_cost - 23.8).abs() < 1e-9);
    assert_eq!(held.size_north, 30.0);
    assert_eq!(held.size_south, 20.0);
}

#[test]
fn strategy_candidate_exposes_per_venue_fields() {
    let strategy = candidate(1_000);
    assert!((strategy.pair_cost() - 0.96).abs() < 1e-9);
    assert_eq!(strategy.instrument_for(Venue::North).as_str(), "north-instrument");
    assert_eq!(strategy.instrument_for(Venue::South).as_str(), "south-instrument");
    assert_eq!(strategy.price_for(Venue::North), 0.46);
    assert_eq!(strategy.price_for(Venue::South), 0.5);
    assert_eq!(strategy.depth_for(Venue::North), 80.0);
    assert_eq!(strategy.depth_for(Venue::South), 40.0);
}

#[test]
fn venue_has_an_opposite() {
    assert_eq!(Venue::North.opposite(), Venue::South);
    assert_eq!(Venue::South.opposite(), Venue::North);
    assert_eq!(Venue::North.to_string(), "NORTH");
    assert_eq!(Venue::South.to_string(), "SOUTH");
}

#[test]
fn quantity_comparisons_use_a_small_epsilon() {
    assert_eq!(QTY_EPSILON, 1e-6);
    assert!(qty_eq(12.0, 12.0 + 1e-9));
    assert!(!qty_eq(12.0, 12.01));
    assert!(qty_at_least(12.0, 12.0));
    assert!(qty_at_least(12.0, 11.5));
    assert!(!qty_at_least(11.5, 12.0));
}

#[test]
fn timestamp_and_duration_arithmetic_is_in_milliseconds() {
    let base = Timestamp::from_seconds(10);
    assert_eq!(base.as_millis(), 10_000);
    assert_eq!((base + Duration::from_seconds(5)).as_millis(), 15_000);
    assert_eq!(
        base.saturating_add(Duration::from_millis(250)).as_millis(),
        10_250
    );
    assert_eq!((Timestamp::from_seconds(40) - base).as_seconds(), 30);
    assert_eq!(Duration::from_millis(1_500).as_seconds(), 1);
}

#[test]
fn cursor_starts_empty_and_round_trips() {
    assert_eq!(Cursor::beginning().as_str(), "");
    assert_eq!(Cursor::default(), Cursor::beginning());
    assert_eq!(Cursor::new("41").as_str(), "41");
    assert_eq!(Cursor::from("7"), Cursor::new(String::from("7")));
}

#[test]
fn engine_config_defaults_are_unchanged() {
    let config = EngineConfig::default();
    assert_eq!(config.max_pair_cost, 0.98);
    assert_eq!(config.min_leg_size, 5.0);
    assert_eq!(config.max_leg_size, 50.0);
    assert_eq!(config.neutralization_slippage, 0.02);
    assert_eq!(config.client_ref_prefix, "pex");
}
