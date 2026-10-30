//! Regression suite: behaviour the task does *not* ask to change.
//!
//! The fix touches fill attribution. This file guards everything adjacent to it,
//! so a candidate cannot reach a green graded suite by loosening a comparison,
//! shifting the exit window, or rewriting position arithmetic. These are pure
//! unit assertions on the frozen contract in `/app/spec/frozen-contract.md`; no
//! engine cycle is driven here.
//!
//! Covered: the cancel lead, the exit window and its inclusive bounds, expiry,
//! position cost arithmetic, the integer quantity comparisons (`qty_eq` and
//! `qty_at_least`, including the one-base-unit boundary), the venue's integer
//! `proportional_share` formula and its truncation toward zero, the attribution
//! window constant and the half-open bounds `PendingOrder` reports, timestamp and
//! duration arithmetic, `Cursor` defaults, and `EngineConfig` defaults.
//!
//! A failure here means the candidate changed something outside the task's scope.

use pairing_engine::types::ids::{proportional_share, qty_at_least, qty_eq, Venue, QTY_UNIT};
use pairing_engine::types::strategy::StrategyCandidate;
use pairing_engine::types::pending_order::{PendingOrder, ATTRIBUTION_WINDOW, CANCEL_LEAD};
use pairing_engine::types::position::{Position, EXIT_WINDOW_END_SECONDS, EXIT_WINDOW_START_SECONDS};
use pairing_engine::types::snapshot::Cursor;
use pairing_engine::types::time::{Duration, Timestamp};
use pairing_engine::EngineConfig;

/// A fixed two-legged candidate whose expiry the caller chooses. Only the expiry
/// varies, so an assertion that changes outcome changed it for that reason.
fn candidate(expiry_seconds: i64) -> StrategyCandidate {
    StrategyCandidate::new(
        "cond".into(),
        "north-instrument".into(),
        "south-instrument".into(),
        0.46,
        0.5,
        80,
        40,
        Timestamp::from_seconds(expiry_seconds),
    )
}

/// A fixed outstanding order on one venue, used to assert the cancel lead and
/// the half-open attribution window it reports.
fn leg(venue: Venue, expiry_seconds: i64) -> PendingOrder {
    PendingOrder::new(
        "cond".into(),
        "north-instrument".into(),
        venue,
        "order-1".into(),
        "pex-1".into(),
        7,
        0.46,
        25,
        Timestamp::from_seconds(expiry_seconds),
        Timestamp::from_seconds(0),
    )
}

/// A fixed position with legs of 30 and 20, used to assert the exit window
/// bounds and the cost arithmetic derived from entry prices and sizes.
fn position(expiry_seconds: i64) -> Position {
    Position::new(
        "cond".into(),
        30,
        20,
        0.46,
        0.5,
        Timestamp::from_seconds(expiry_seconds),
        Timestamp::from_seconds(0),
    )
}

/// Orders become cancellable five minutes before expiry.
///
/// The lead is part of the frozen contract. An engine may not shift when the
/// venue will cancel a resting order, because attribution depends on knowing
/// which orders are still alive.
#[test]
fn cancel_lead_is_five_minutes() {
    assert_eq!(CANCEL_LEAD.as_seconds(), 300);
}

/// `should_cancel` is false at 699 and true from 701, for an order expiring at
/// 1000.
///
/// Widening this to retire orders earlier removes them from tracking before their
/// fills are attributed, which turns a graded attribution failure into a
/// disappeared order. Narrowing it strands orders past their cancel point.
#[test]
fn pending_order_cancels_only_inside_the_cancel_lead() {
    let order = leg(Venue::North, 1_000);
    assert!(!order.should_cancel(Timestamp::from_seconds(699)));
    assert!(order.should_cancel(Timestamp::from_seconds(701)));
    assert!(order.should_cancel(Timestamp::from_seconds(1_000)));
    assert!(order.should_cancel(Timestamp::from_seconds(1_400)));
}

/// Time to expiry is signed, and goes negative once expiry has passed.
///
/// Saturating this at zero would make an expired order indistinguishable from one
/// expiring exactly now.
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

/// The exit window opens 120 seconds before expiry and closes 60 seconds before.
///
/// Position exit is outside the task's scope entirely. These two constants pin it
/// so a rewrite of position handling shows up here rather than as noise in the
/// graded suite.
#[test]
fn exit_window_bounds_are_unchanged() {
    assert_eq!(EXIT_WINDOW_START_SECONDS, 60);
    assert_eq!(EXIT_WINDOW_END_SECONDS, 120);
}

/// Both ends of the exit window are inclusive: 880 and 940 are inside, 879 and
/// 941 are not.
///
/// Deliberately the opposite convention to the half-open attribution window, and
/// both are tested to the second. The two must not be conflated: an engine that
/// standardises on one convention breaks the other.
#[test]
fn position_exit_window_is_inclusive_at_both_ends() {
    let held = position(1_000);
    assert!(!held.should_exit_now(Timestamp::from_seconds(879)));
    assert!(held.should_exit_now(Timestamp::from_seconds(880)));
    assert!(held.should_exit_now(Timestamp::from_seconds(920)));
    assert!(held.should_exit_now(Timestamp::from_seconds(940)));
    assert!(!held.should_exit_now(Timestamp::from_seconds(941)));
}

/// Expiry is reached at the expiry instant itself, not one second later.
///
/// The final-minute flag and the expiry test are separate predicates with
/// different boundaries, and both are pinned.
#[test]
fn position_flags_the_final_minute_and_expiry() {
    let held = position(1_000);
    assert!(!held.is_too_close_to_expiry(Timestamp::from_seconds(940)));
    assert!(held.is_too_close_to_expiry(Timestamp::from_seconds(941)));
    assert!(!held.is_expired(Timestamp::from_seconds(999)));
    assert!(held.is_expired(Timestamp::from_seconds(1_000)));
    assert!(held.is_expired(Timestamp::from_seconds(1_001)));
}

/// Cost is entry price times size per leg, summed.
///
/// The graded suite asserts costs on positions it expects, so this pins the
/// arithmetic independently. A candidate cannot reach the expected cost figures by
/// changing how cost is computed rather than by attributing the right legs.
///
/// Prices are floating point and compared within 1e-9; quantities are not, and
/// never are.
#[test]
fn position_costs_are_derived_from_entry_prices_and_sizes() {
    let held = position(1_000);
    assert!((held.pair_cost - 0.96).abs() < 1e-9);
    assert!((held.total_notional_cost - 23.8).abs() < 1e-9);
    assert_eq!(held.size_north, 30);
    assert_eq!(held.size_south, 20);
}

/// A candidate's per-venue accessors return that venue's own instrument, price
/// and depth.
///
/// North and South are asserted separately and hold different values, so a
/// transposed match arm is visible here rather than as a mysterious cross-venue
/// attribution failure in the graded suite.
#[test]
fn strategy_candidate_exposes_per_venue_fields() {
    let strategy = candidate(1_000);
    assert!((strategy.pair_cost() - 0.96).abs() < 1e-9);
    assert_eq!(strategy.instrument_for(Venue::North).as_str(), "north-instrument");
    assert_eq!(strategy.instrument_for(Venue::South).as_str(), "south-instrument");
    assert_eq!(strategy.price_for(Venue::North), 0.46);
    assert_eq!(strategy.price_for(Venue::South), 0.5);
    assert_eq!(strategy.depth_for(Venue::North), 80);
    assert_eq!(strategy.depth_for(Venue::South), 40);
}

/// `Venue::opposite` pairs the two venues, and the display strings are stable.
///
/// Neutralization is placed on the opposite venue, so an identity `opposite` sends
/// every corrective order to the venue that already filled.
#[test]
fn venue_has_an_opposite() {
    assert_eq!(Venue::North.opposite(), Venue::South);
    assert_eq!(Venue::South.opposite(), Venue::North);
    assert_eq!(Venue::North.to_string(), "NORTH");
    assert_eq!(Venue::South.to_string(), "SOUTH");
}

/// Quantity comparison is exact at the base unit, in both directions.
///
/// This is the test that forecloses the tempting shortcut. The original defect
/// shipped with a `FILL_TOLERANCE`, and reintroducing any tolerance — even one base
/// unit — to make a leg "close enough" to filled fails here directly.
#[test]
fn quantity_comparisons_are_exact_with_no_tolerance() {
    assert_eq!(QTY_UNIT, 1);
    assert!(qty_eq(12, 12));
    // One base unit apart is not equal. There is no tolerance to hide in.
    assert!(!qty_eq(12, 12 + QTY_UNIT));
    assert!(!qty_eq(12, 12 - QTY_UNIT));
    assert!(qty_at_least(12, 12));
    assert!(qty_at_least(12, 11));
    // Short by exactly one base unit is short.
    assert!(!qty_at_least(12 - QTY_UNIT, 12));
}

/// The venue's share formula truncates toward zero and never rounds.
///
/// This is the arithmetic half of the block division rule; the graded suite covers
/// the placement of the leftover units. The third assertion is the reason the two
/// halves are separate: three shares of a 37-unit block total 36, and the missing
/// base unit is the caller's to place. A candidate that rounds here will appear to
/// conserve the total while assigning the wrong order the extra unit.
#[test]
fn proportional_share_truncates_toward_zero() {
    // 37 * 10 / 50 = 7.4 -> 7. Never 7.4, never 8.
    assert_eq!(proportional_share(37, 10, 50), 7);
    assert_eq!(proportional_share(37, 30, 50), 22);
    // The three shares total 36, one short of the 37 that was matched. The
    // leftover base unit is placed by the caller, not by this formula.
    assert_eq!(
        proportional_share(37, 10, 50)
            + proportional_share(37, 10, 50)
            + proportional_share(37, 30, 50),
        36
    );
    // A share that would round up under floating-point arithmetic truncates.
    assert_eq!(proportional_share(10, 3, 4), 7);
    assert_eq!(proportional_share(1, 1, 3), 0);
}

/// Every degenerate input yields zero rather than a panic or a negative share.
///
/// A zero basis is the case that matters: an eligible set whose outstanding
/// quantities sum to zero must attribute nothing, and must not divide by zero.
/// Negative inputs cannot arise from a well-formed venue message and are pinned to
/// zero so they cannot become negative attribution.
#[test]
fn proportional_share_allocates_nothing_without_a_basis() {
    assert_eq!(proportional_share(40, 10, 0), 0);
    assert_eq!(proportional_share(40, 10, -5), 0);
    assert_eq!(proportional_share(0, 10, 50), 0);
    assert_eq!(proportional_share(-40, 10, 50), 0);
    assert_eq!(proportional_share(40, 0, 50), 0);
    assert_eq!(proportional_share(40, -10, 50), 0);
}

/// The window is 900 seconds, and `PendingOrder` reports bounds exactly that far
/// apart.
///
/// The constant and the reported bounds are frozen; the half-open comparison
/// against them is the candidate's to write, and the graded suite tests it at both
/// edges. Shifting the constant or the bounds to make a boundary case pass would
/// move every window at once, and fails here.
#[test]
fn attribution_window_is_fifteen_minutes_and_half_open() {
    assert_eq!(ATTRIBUTION_WINDOW.as_seconds(), 900);
    let order = leg(Venue::North, 100_000);
    assert_eq!(order.attribution_start(), Timestamp::from_seconds(0));
    assert_eq!(order.attribution_end(), Timestamp::from_seconds(900));
    // The reported bounds are exactly one window apart, so the half-open rule
    // `[start, end)` is decided by the comparison and not by the constant.
    assert_eq!(
        order.attribution_end() - order.attribution_start(),
        ATTRIBUTION_WINDOW
    );
}

/// Timestamps and durations are milliseconds internally, and `as_seconds`
/// truncates.
///
/// Window comparisons are made against these values, so the unit and the
/// truncation direction both matter: 1500 milliseconds is 1 second, not 2.
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

/// The beginning cursor is the empty string, and is also the default.
///
/// The candidate is expected to persist a cursor across restarts. This pins what
/// "no progress yet" is, so a restored cursor can be distinguished from an absent
/// one.
#[test]
fn cursor_starts_empty_and_round_trips() {
    assert_eq!(Cursor::beginning().as_str(), "");
    assert_eq!(Cursor::default(), Cursor::beginning());
    assert_eq!(Cursor::new("41").as_str(), "41");
    assert_eq!(Cursor::from("7"), Cursor::new(String::from("7")));
}

/// Config defaults, including the leg size bounds and the client-ref prefix.
///
/// Sizing is outside the task's scope. Changing `min_leg_size` or `max_leg_size`
/// would alter which candidates are entered at all and shift every expected
/// quantity in the graded suite, so the defaults are pinned here.
#[test]
fn engine_config_defaults_are_unchanged() {
    let config = EngineConfig::default();
    assert_eq!(config.max_pair_cost, 0.98);
    assert_eq!(config.min_leg_size, 5);
    assert_eq!(config.max_leg_size, 50);
    assert_eq!(config.neutralization_slippage, 0.02);
    assert_eq!(config.client_ref_prefix, "pex");
}
