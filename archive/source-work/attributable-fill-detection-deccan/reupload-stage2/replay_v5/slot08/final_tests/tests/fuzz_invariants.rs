//! Randomised invariants over the whole surface.
//!
//! Not graded. Runs seeded scenarios — restarts, feed redelivery, withheld
//! events, lost acknowledgements, refused and failed submissions — and checks
//! what must hold whatever the venue did: a pairing is recorded only when both
//! legs really filled, an exposure is never corrected twice over, and the same
//! run replays identically.
//!
//! Block matches are excluded: the fake venue credits a block to custody without
//! marking it on the orders it covered, so it has no per-order quantity to
//! compare against.

mod harness;

use harness::*;

const SEEDS: u64 = 400;

fn scenario_for(seed: u64) -> Scenario {
    let mut prng = Prng::new(seed);
    let mandates = 1 + prng.below(3);
    let mut candidates = Vec::new();
    for index in 0..mandates {
        let size = 5 + prng.below(26) as Qty;
        candidates.push(candidate(
            &format!("s{index}"),
            &format!("n{index}"),
            &format!("t{index}"),
            0.45,
            0.52,
            size,
            size,
        ));
    }
    let mut scenario = Scenario::new(candidates);
    for index in 0..mandates {
        scenario = scenario
            .ask(instrument(&format!("n{index}")), 0.47)
            .ask(instrument(&format!("t{index}")), 0.54);
    }
    let steps = 14 + prng.below(8);
    for _ in 0..steps {
        let mut actions: Vec<Action> = Vec::new();
        for _ in 0..prng.below(3) {
            let mandate = format!("s{}", prng.below(mandates));
            let venue = if prng.below(2) == 0 {
                Venue::North
            } else {
                Venue::South
            };
            let qty = 1 + prng.below(30) as Qty;
            match prng.below(14) {
                0 | 1 | 2 | 3 => actions.push(fill_resting(&mandate, venue, qty)),
                4 => actions.push(fill_taker(&mandate, venue, qty)),
                5 => actions.push(Action::RedeliverAllFillEvents),
                6 => actions.push(Action::WithholdFillEvents),
                7 => actions.push(Action::ReleaseFillEvents),
                8 => actions.push(Action::LoseNextAcknowledgement),
                9 => actions.push(Action::RejectNextTakerSubmission),
                10 => actions.push(Action::FailNextCancel),
                11 => actions.push(Action::FailNextFillPoll),
                12 => actions.push(Action::DisableTakerAutofill),
                _ => actions.push(Action::Restart),
            }
        }
        scenario = scenario.step(actions);
    }
    scenario
}

/// What the venue really filled for one mandate on one venue, across the resting
/// order and every corrective order the engine sent there.
fn truly_filled(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Qty {
    outcome
        .venue_orders
        .iter()
        .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
        .map(|order| order.filled_qty)
        .sum()
}

#[test]
fn a_pairing_is_recorded_only_when_both_legs_filled() {
    for seed in 0..SEEDS {
        let outcome = run(scenario_for(seed));
        for position in &outcome.positions {
            let id = position.strategy_id.as_str();
            let north = truly_filled(&outcome, id, Venue::North);
            let south = truly_filled(&outcome, id, Venue::South);
            assert!(
                north >= position.size_north && south >= position.size_south,
                "seed {seed}: a pairing of {} north / {} south was recorded on fills of {} / {}: \
                 mandate {id}",
                position.size_north,
                position.size_south,
                north,
                south
            );
        }
    }
}

#[test]
fn an_exposure_is_never_corrected_beyond_what_it_was_short() {
    for seed in 0..SEEDS {
        let outcome = run(scenario_for(seed));
        for position in &outcome.positions {
            let id = position.strategy_id.as_str();
            for venue in [Venue::North, Venue::South] {
                let size = if venue == Venue::North {
                    position.size_north
                } else {
                    position.size_south
                };
                let corrected: Qty = outcome
                    .takers_for(id, venue)
                    .iter()
                    .map(|order| order.size)
                    .sum();
                let resting: Qty = outcome
                    .resting_for(id, venue)
                    .iter()
                    .map(|order| order.filled_qty)
                    .sum();
                assert!(
                    resting + corrected >= size,
                    "seed {seed}: mandate {id} on {venue} needed {size} and was worked for \
                     {resting} resting plus {corrected} corrected, yet a pairing was recorded"
                );
            }
        }
    }
}

#[test]
fn the_same_observations_replay_to_the_same_outcome() {
    for seed in 0..SEEDS / 4 {
        let one = run(scenario_for(seed));
        let two = run(scenario_for(seed));
        let summarise = |outcome: &Outcome| {
            (
                outcome.positions.clone(),
                outcome.pending_orders.clone(),
                outcome.venue_log.clone(),
                outcome.cancelled_order_ids.clone(),
            )
        };
        assert_eq!(
            summarise(&one),
            summarise(&two),
            "seed {seed}: the same observations in the same order reached a different outcome"
        );
    }
}
