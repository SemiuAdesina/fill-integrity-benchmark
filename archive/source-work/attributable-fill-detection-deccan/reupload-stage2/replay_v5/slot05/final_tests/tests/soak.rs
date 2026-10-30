//! A soak run: many randomised scenarios, checking the engine stays within the
//! contract's bounds rather than pinning one exact outcome.
//!
//! Nothing here is graded. The point is to catch a panic, an unbounded stream of
//! corrective orders, or a pairing recorded without both legs having filled.

mod harness;

use harness::*;

/// One randomised scenario, built from a seed so it replays identically.
fn soak(seed: u64) -> Outcome {
    let mut prng = Prng::new(seed);
    let ids = ["s0", "s1", "s2"];
    let mut candidates = Vec::new();
    for (index, id) in ids.iter().enumerate() {
        let south = if index == 0 { "tS" } else { "tX" };
        candidates.push(candidate(
            id,
            &format!("n{}", index),
            south,
            0.44 + index as Price * 0.01,
            0.50,
            10 + 5 * index as Qty,
            10 + 5 * index as Qty,
        ));
    }
    let mut scenario = Scenario::new(candidates)
        .ask(instrument("tS"), 0.54)
        .ask(instrument("tX"), 0.54);
    for index in 0..3 {
        scenario = scenario.ask(instrument(&format!("n{}", index)), 0.46);
    }

    let steps = 26;
    for cycle in 0..steps {
        let mut actions: Vec<Action> = Vec::new();
        if cycle == 0 && prng.below(2) == 0 {
            actions.push(Action::DisableTakerAutofill);
        }
        // Keep the venue busy: legs only complete if something fills them.
        if prng.below(10) < 7 {
            let id = ids[prng.below(ids.len())];
            let venue = if prng.below(2) == 0 { Venue::North } else { Venue::South };
            actions.push(fill_resting(id, venue, 4 + prng.below(9) as Qty));
        }
        if prng.below(10) < 4 {
            let id = ids[prng.below(ids.len())];
            let venue = if prng.below(2) == 0 { Venue::North } else { Venue::South };
            actions.push(fill_taker(id, venue, 3 + prng.below(8) as Qty));
        }
        if prng.below(10) < 3 {
            actions.push(block_takers(
                &[ids[0], ids[1]],
                Venue::South,
                4 + prng.below(24) as Qty,
                0.55,
            ));
        }
        match prng.below(8) {
            0 => actions.push(Action::RedeliverAllFillEvents),
            1 => actions.push(Action::RedeliverAllBlocks),
            2 => actions.push(Action::Restart),
            3 => actions.push(Action::LoseNextAcknowledgement),
            4 => actions.push(Action::RejectNextTakerSubmission),
            5 => actions.push(Action::FailNextFillPoll),
            6 => actions.push(Action::FailNextCancel),
            _ => {}
        }
        scenario = scenario.step(actions);
    }
    run(scenario)
}

#[test]
fn randomised_runs_never_panic_or_run_away() {
    let mut worst = 0usize;
    for seed in 0..120u64 {
        let outcome = soak(seed);
        assert!(
            outcome.cycle_errors.is_empty(),
            "seed {seed}: cycles returned errors: {:?}",
            outcome.cycle_errors
        );
        for order in &outcome.venue_orders {
            assert!(
                order.filled_qty <= order.size,
                "seed {seed}: the venue filled {} of an order of {}",
                order.filled_qty,
                order.size
            );
        }
        // A pairing is only recorded when both halves were acquired.
        for position in &outcome.positions {
            assert!(
                outcome.acquired_at_most(position.strategy_id.as_str(), Venue::North)
                    >= position.size_north
                    && outcome.acquired_at_most(position.strategy_id.as_str(), Venue::South)
                        >= position.size_south,
                "seed {seed}: {} was paired with North {} of {} and South {} of {}",
                position.strategy_id.as_str(),
                outcome.acquired_at_most(position.strategy_id.as_str(), Venue::North),
                position.size_north,
                outcome.acquired_at_most(position.strategy_id.as_str(), Venue::South),
                position.size_south
            );
        }
        // Nothing acquires more than the leg's size, however often the feed
        // replays, the process restarts, or a block closes an order early.
        for id in ["s0", "s1", "s2"] {
            for venue in [Venue::North, Venue::South] {
                let filled: Qty = outcome
                    .venue_orders
                    .iter()
                    .filter(|order| order.strategy_id.as_str() == id && order.venue == venue)
                    .map(|order| order.filled_qty)
                    .sum();
                let size = outcome
                    .venue_orders
                    .iter()
                    .filter(|order| order.strategy_id.as_str() == id && order.venue == venue)
                    .map(|order| order.size)
                    .max()
                    .unwrap_or(0);
                assert!(
                    filled <= size,
                    "seed {seed}: {id} acquired {filled} on {venue:?} against a leg of {size}"
                );
            }
        }
        // One exposure draws a bounded number of corrective orders, however
        // often the feed replays or the process restarts.
        for order in &outcome.venue_orders {
            let count = outcome
                .takers_for(order.strategy_id.as_str(), order.venue)
                .len();
            if count > worst { worst = count; }
            assert!(
                count <= 4,
                "seed {seed}: {} corrective orders for {} on {:?}",
                count,
                order.strategy_id.as_str(),
                order.venue
            );
        }
    }
    println!("worst corrective-order count for one exposure: {worst}");
    let mut positions = 0usize;
    let mut takers = 0usize;
    for seed in 0..120u64 {
        let outcome = soak(seed);
        positions += outcome.positions.len();
        takers += outcome.taker_orders().len();
    }
    println!("positions across seeds: {positions}, corrective orders: {takers}");
}
