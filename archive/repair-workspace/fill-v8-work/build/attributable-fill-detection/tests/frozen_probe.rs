//! Candidate-linked frozen-contract observation probe.
//!
//! Each arm evaluates one documented public behavior and writes only the raw
//! values it observed. It contains no expected values, assertions, verdicts or
//! reward logic. The separate verifier-owned judge performs all comparisons.

use pairing_engine::types::ids::{proportional_share, qty_at_least, qty_eq, Venue, QTY_UNIT};
use pairing_engine::types::pending_order::{PendingOrder, ATTRIBUTION_WINDOW, CANCEL_LEAD};
use pairing_engine::types::position::{Position, EXIT_WINDOW_END_SECONDS, EXIT_WINDOW_START_SECONDS};
use pairing_engine::types::snapshot::Cursor;
use pairing_engine::types::strategy::StrategyCandidate;
use pairing_engine::types::time::{Duration, Timestamp};
use pairing_engine::EngineConfig;

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

fn observe(case: &str) -> Option<String> {
    Some(match case {
        // Public behavior: cancellation lead. Rejects shifted timing constants.
        "cancel_lead_is_five_minutes" => format!("{}\n", CANCEL_LEAD.as_seconds()),
        // Public behavior: cancel boundary. Rejects inclusive/shifted comparisons.
        "pending_order_cancels_only_inside_the_cancel_lead" => {
            let order = leg(Venue::North, 1_000);
            format!(
                "{}|{}|{}|{}\n",
                order.should_cancel(Timestamp::from_seconds(699)),
                order.should_cancel(Timestamp::from_seconds(701)),
                order.should_cancel(Timestamp::from_seconds(1_000)),
                order.should_cancel(Timestamp::from_seconds(1_400))
            )
        }
        // Public behavior: signed time-to-expiry. Rejects saturating arithmetic.
        "pending_order_reports_remaining_seconds" => {
            let order = leg(Venue::South, 900);
            format!(
                "{}|{}\n",
                order.seconds_until_expiry(Timestamp::from_seconds(300)),
                order.seconds_until_expiry(Timestamp::from_seconds(960))
            )
        }
        // Public behavior: fixed exit-window constants. Rejects shifted windows.
        "exit_window_bounds_are_unchanged" => {
            format!("{}|{}\n", EXIT_WINDOW_START_SECONDS, EXIT_WINDOW_END_SECONDS)
        }
        // Public behavior: inclusive exit bounds. Rejects half-open substitution.
        "position_exit_window_is_inclusive_at_both_ends" => {
            let held = position(1_000);
            format!(
                "{}|{}|{}|{}|{}\n",
                held.should_exit_now(Timestamp::from_seconds(879)),
                held.should_exit_now(Timestamp::from_seconds(880)),
                held.should_exit_now(Timestamp::from_seconds(920)),
                held.should_exit_now(Timestamp::from_seconds(940)),
                held.should_exit_now(Timestamp::from_seconds(941))
            )
        }
        // Public behavior: final-minute and expiry predicates. Rejects conflation.
        "position_flags_the_final_minute_and_expiry" => {
            let held = position(1_000);
            format!(
                "{}|{}|{}|{}|{}\n",
                held.is_too_close_to_expiry(Timestamp::from_seconds(940)),
                held.is_too_close_to_expiry(Timestamp::from_seconds(941)),
                held.is_expired(Timestamp::from_seconds(999)),
                held.is_expired(Timestamp::from_seconds(1_000)),
                held.is_expired(Timestamp::from_seconds(1_001))
            )
        }
        // Public behavior: position arithmetic. Rejects transposed sizes/prices.
        "position_costs_are_derived_from_entry_prices_and_sizes" => {
            let held = position(1_000);
            format!(
                "{:.12}|{:.12}|{}|{}\n",
                held.pair_cost, held.total_notional_cost, held.size_north, held.size_south
            )
        }
        // Public behavior: per-venue strategy accessors. Rejects swapped match arms.
        "strategy_candidate_exposes_per_venue_fields" => {
            let strategy = candidate(1_000);
            format!(
                "{:.12}|{}|{}|{:.12}|{:.12}|{}|{}\n",
                strategy.pair_cost(),
                strategy.instrument_for(Venue::North).as_str(),
                strategy.instrument_for(Venue::South).as_str(),
                strategy.price_for(Venue::North),
                strategy.price_for(Venue::South),
                strategy.depth_for(Venue::North),
                strategy.depth_for(Venue::South)
            )
        }
        // Public behavior: venue pairing/display. Rejects identity opposite mapping.
        "venue_has_an_opposite" => format!(
            "{:?}|{:?}|{}|{}\n",
            Venue::North.opposite(),
            Venue::South.opposite(),
            Venue::North,
            Venue::South
        ),
        // Public behavior: exact quantity predicates. Rejects any epsilon.
        "quantity_comparisons_are_exact_with_no_tolerance" => format!(
            "{}|{}|{}|{}|{}|{}|{}\n",
            QTY_UNIT,
            qty_eq(12, 12),
            qty_eq(12, 12 + QTY_UNIT),
            qty_eq(12, 12 - QTY_UNIT),
            qty_at_least(12, 12),
            qty_at_least(12, 11),
            qty_at_least(12 - QTY_UNIT, 12)
        ),
        // Public behavior: integer proportional shares. Rejects rounding.
        "proportional_share_truncates_toward_zero" => format!(
            "{}|{}|{}|{}|{}\n",
            proportional_share(37, 10, 50),
            proportional_share(37, 30, 50),
            proportional_share(37, 10, 50) * 2 + proportional_share(37, 30, 50),
            proportional_share(10, 3, 4),
            proportional_share(1, 1, 3)
        ),
        // Public behavior: degenerate share inputs. Rejects panic/negative output.
        "proportional_share_allocates_nothing_without_a_basis" => format!(
            "{}|{}|{}|{}|{}|{}\n",
            proportional_share(40, 10, 0),
            proportional_share(40, 10, -5),
            proportional_share(0, 10, 50),
            proportional_share(-40, 10, 50),
            proportional_share(40, 0, 50),
            proportional_share(40, -10, 50)
        ),
        // Public behavior: attribution window duration/bounds. Rejects shifted bounds.
        "attribution_window_is_fifteen_minutes_and_half_open" => {
            let order = leg(Venue::North, 100_000);
            format!(
                "{}|{}|{}|{}\n",
                ATTRIBUTION_WINDOW.as_seconds(),
                order.attribution_start().as_millis(),
                order.attribution_end().as_millis(),
                (order.attribution_end() - order.attribution_start()).as_millis()
            )
        }
        // Public behavior: millisecond arithmetic/truncation. Rejects wrong units.
        "timestamp_and_duration_arithmetic_is_in_milliseconds" => {
            let base = Timestamp::from_seconds(10);
            format!(
                "{}|{}|{}|{}|{}\n",
                base.as_millis(),
                (base + Duration::from_seconds(5)).as_millis(),
                base.saturating_add(Duration::from_millis(250)).as_millis(),
                (Timestamp::from_seconds(40) - base).as_seconds(),
                Duration::from_millis(1_500).as_seconds()
            )
        }
        // Public behavior: cursor defaults/conversion. Rejects nonempty defaults.
        "cursor_starts_empty_and_round_trips" => format!(
            "{}|{}|{}|{}\n",
            Cursor::beginning().as_str(),
            Cursor::default().as_str(),
            Cursor::new("41").as_str(),
            Cursor::from("7").as_str()
        ),
        // Public behavior: engine defaults. Rejects out-of-scope sizing changes.
        "engine_config_defaults_are_unchanged" => {
            let config = EngineConfig::default();
            format!(
                "{:.12}|{}|{}|{:.12}|{}\n",
                config.max_pair_cost,
                config.min_leg_size,
                config.max_leg_size,
                config.neutralization_slippage,
                config.client_ref_prefix
            )
        }
        _ => return None,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        std::process::exit(2);
    }
    let Some(raw) = observe(&args[1]) else {
        std::process::exit(2);
    };
    if raw.len() > 4096 || std::fs::write(&args[2], raw).is_err() {
        std::process::exit(2);
    }
}
