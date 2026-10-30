//! Randomised invariants.
//!
//! Scenarios are generated from a seed and replayed identically. Nothing here
//! names an internal type: every assertion is about what the venue saw or what
//! the engine reports about itself.
//!
//! Rejections and blocks are deliberately left out of the generator. Both
//! legitimately end one corrective order and start another, so a count of one
//! per exposure would not hold; everything else must hold.

mod harness;

use harness::*;
use std::collections::BTreeSet;

const STRATEGY: &str = "fz";

fn filled_at_venue(outcome: &Outcome, strategy_id: &str, venue: Venue) -> Qty {
    outcome
        .venue_orders
        .iter()
        .filter(|order| order.strategy_id.as_str() == strategy_id && order.venue == venue)
        .map(|order| order.filled_qty)
        .sum()
}

fn scenario_for(seed: u64) -> Scenario {
    let mut rng = Prng::new(seed);
    let north_size = 10 + rng.below(21) as Qty;
    let south_size = 10 + rng.below(21) as Qty;
    let mut scenario = Scenario::new(vec![candidate(
        STRATEGY,
        "nZ",
        "tZ",
        0.45,
        0.52,
        north_size,
        south_size,
    )])
    .ask(instrument("nZ"), 0.47)
    .ask(instrument("tZ"), 0.54);

    let mut steps: Vec<Vec<Action>> = Vec::new();
    if rng.below(2) == 0 {
        steps.push(vec![Action::DisableTakerAutofill]);
    } else {
        steps.push(vec![]);
    }
    for _ in 0..14 {
        let mut actions: Vec<Action> = Vec::new();
        match rng.below(12) {
            0 | 1 | 2 | 3 => {
                let venue = if rng.below(2) == 0 { Venue::North } else { Venue::South };
                let qty = 1 + rng.below(31) as Qty;
                actions.push(fill_resting(STRATEGY, venue, qty));
            }
            4 => actions.push(Action::WithholdFillEvents),
            5 => actions.push(Action::ReleaseFillEvents),
            6 => actions.push(Action::RedeliverAllFillEvents),
            7 => actions.push(Action::Restart),
            8 => actions.push(Action::LoseNextAcknowledgement),
            9 => actions.push(Action::FailNextFillPoll),
            10 => actions.push(Action::FailNextCancel),
            _ => {}
        }
        steps.push(actions);
    }
    // Whatever happened, the run ends with quiet cycles: nothing new may be
    // sent once the world has stopped moving.
    for _ in 0..8 {
        steps.push(vec![]);
    }
    for actions in steps {
        scenario = scenario.step(actions);
    }
    scenario
}

fn check(seed: u64) {
    let outcome = run(scenario_for(seed));

    let mut references: BTreeSet<String> = BTreeSet::new();
    for order in &outcome.venue_orders {
        assert!(
            references.insert(order.client_ref.as_str().to_string()),
            "seed {seed}: the venue holds two orders under client reference {}",
            order.client_ref.as_str()
        );
    }

    for venue in [Venue::North, Venue::South] {
        let takers = outcome.takers_for(STRATEGY, venue);
        assert!(
            takers.len() <= 1,
            "seed {seed}: {} corrective orders for one exposure on {venue}: {:?}",
            takers.len(),
            takers.iter().map(|order| order.size).collect::<Vec<_>>()
        );
    }

    for position in &outcome.positions {
        assert!(
            filled_at_venue(&outcome, STRATEGY, Venue::North) >= position.size_north
                && filled_at_venue(&outcome, STRATEGY, Venue::South) >= position.size_south,
            "seed {seed}: a pairing of {} x {} was recorded for fills of {} and {}",
            position.size_north,
            position.size_south,
            filled_at_venue(&outcome, STRATEGY, Venue::North),
            filled_at_venue(&outcome, STRATEGY, Venue::South)
        );
    }
}

#[test]
fn one_corrective_order_per_exposure_holds_across_random_runs() {
    for seed in 0..200u64 {
        check(seed);
    }
}
