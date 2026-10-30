//! Diagnostics: randomised runs, checked against invariants rather than fixtures.
//!
//! Every scenario is generated from a seed and checked for two things the contract
//! requires everywhere: the venue never matches more than a leg's size (which is
//! what sending the same exposure twice would show up as), and the same
//! observations in the same order reach the same observable outcome. None of this
//! is graded.

mod harness;

use harness::*;

/// Total base units the venue matched for one strategy on one venue, across the
/// resting order and every corrective order. Over-commitment shows up here: the
/// venue never fills more than an order's size, so a figure above the leg size
/// means the engine sent the same exposure twice.
fn acquired(outcome: &Outcome, s: &str, v: Venue) -> Qty {
    outcome.venue_orders.iter()
        .filter(|o| o.strategy_id.as_str() == s && o.venue == v)
        .map(|o| o.filled_qty).sum()
}

fn check(scenario: &Scenario) -> Outcome {
    let outcome = run(scenario.clone());
    for candidate in &scenario.candidates {
        let id = candidate.strategy_id.as_str();
        let size = candidate.depth_for(Venue::North);
        let got = acquired(&outcome, id, Venue::North);
        assert!(got <= size, "{id}/{:?}: acquired {got} of a leg of {size}", Venue::North);
        let size_s = candidate.depth_for(Venue::South);
        let got_s = acquired(&outcome, id, Venue::South);
        assert!(got_s <= size_s, "{id}/{:?}: acquired {got_s} of a leg of {size_s}", Venue::South);
        if let Some(position) = outcome.position(id) {
            assert!(got >= position.size_north && got_s >= position.size_south,
                "{id}: a pairing was recorded on legs of {got}/{got_s}");
        }
        assert!(outcome.cycle_errors.is_empty(), "{id}: cycle errors {:?}", outcome.cycle_errors);
    }
    outcome
}

#[test]
fn randomized_scenarios_never_over_commit_and_replay_identically() {
    let instruments = ["nA", "nB", "tA", "tB", "tS"];
    for seed in 1..=120u64 {
        let mut prng = Prng::new(seed);
        let strategy_count = 1 + prng.below(3);
        let mut candidates = Vec::new();
        for index in 0..strategy_count {
            let north = instruments[prng.below(instruments.len())];
            let south = instruments[prng.below(instruments.len())];
            let size = 5 + prng.below(20) as Qty;
            candidates.push(candidate(
                &format!("s{index}"), north, south, 0.45, 0.52, size, size,
            ));
        }
        let mut scenario = Scenario::new(candidates.clone())
            .with_cycle_seconds([0, 10, 30, 60][prng.below(4)] as i64);
        for instrument_id in &instruments {
            scenario = scenario.ask(instrument(instrument_id), 0.47);
        }
        let mut steps: Vec<Vec<Action>> = Vec::new();
        let step_count = 8 + prng.below(8);
        for cycle in 0..step_count {
            let mut actions: Vec<Action> = Vec::new();
            for _ in 0..prng.below(3) {
                let candidate_index = prng.below(candidates.len());
                let strategy_id = candidates[candidate_index].strategy_id.clone();
                let venue = if prng.below(2) == 0 { Venue::North } else { Venue::South };
                match prng.below(11) {
                    0 => actions.push(fill_resting(strategy_id.as_str(), venue, 1 + prng.below(12) as Qty)),
                    1 => actions.push(fill_taker(strategy_id.as_str(), venue, 1 + prng.below(12) as Qty)),
                    2 => actions.push(Action::RedeliverAllFillEvents),
                    3 => actions.push(Action::RedeliverAllBlocks),
                    4 => actions.push(Action::WithholdFillEvents),
                    5 => actions.push(Action::ReleaseFillEvents),
                    6 => actions.push(Action::LoseNextAcknowledgement),
                    7 => actions.push(Action::RejectNextTakerSubmission),
                    8 => actions.push(Action::FailNextCancel),
                    9 => actions.push(Action::Restart),
                    _ => {
                        let covered: Vec<OrderRef> = (0..=prng.below(2)).map(|offset| {
                            let index = (candidate_index + offset) % candidates.len();
                            OrderRef::Taker { strategy: candidates[index].strategy_id.clone(), venue }
                        }).collect();
                        actions.push(block_orders(covered, 1 + prng.below(30) as Qty, 0.55, 0));
                    }
                }
            }
            if cycle == 3 {
                actions.push(Action::DisableTakerAutofill);
            }
            steps.push(actions);
        }
        for actions in steps {
            scenario = scenario.step(actions);
        }
        let first = check(&scenario);
        let second = run(scenario.clone());
        assert_eq!(first.venue_orders, second.venue_orders, "seed {seed}: the same observations gave a different outcome");
        assert_eq!(first.positions, second.positions, "seed {seed}: the same observations gave different pairings");
    }
}
